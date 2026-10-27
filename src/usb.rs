//! A USB serial port, run from a high-priority interrupt executor so it stays
//! responsive while the main loop renders. Once a second it reports the frame
//! timings; it takes the commands `bootsel` (reboot into the USB bootloader to
//! be flashed), `screen` (switch screens) and `press up|down|enter`.

use core::cell::Cell;
use core::fmt::Write as _;
use core::sync::atomic::{AtomicBool, AtomicU8, Ordering};

use alloc::string::String;
use badger_ratatui::input::{DOWN, ENTER, UP};
use embassy_executor::InterruptExecutor;
use embassy_futures::join::join;
use embassy_rp::interrupt;
use embassy_rp::interrupt::{InterruptExt, Priority};
use embassy_rp::peripherals::USB;
use embassy_rp::usb::{Driver, InterruptHandler};
use embassy_rp::{Peri, bind_interrupts};
use embassy_sync::blocking_mutex::CriticalSectionMutex;
use embassy_time::Timer;
use embassy_usb::class::cdc_acm::{CdcAcmClass, Receiver, Sender, State};
use embassy_usb::{Builder, Config};
use static_cell::StaticCell;

bind_interrupts!(struct Irqs {
    USBCTRL_IRQ => InterruptHandler<USB>;
});

static EXECUTOR: InterruptExecutor = InterruptExecutor::new();

#[interrupt]
unsafe fn SWI_IRQ_1() {
    // SAFETY: the executor's own interrupt, started in `start` before it is unmasked.
    unsafe { EXECUTOR.on_interrupt() }
}

/// Mean frame timings over the last report period, in microseconds.
#[derive(Clone, Copy, Default)]
pub struct Stats {
    pub frames: u32,
    /// Input and simulation.
    pub update_us: u64,
    /// The screen's own drawing into the ratatui buffer (the showcase's shading).
    pub render_us: u64,
    /// Ratatui's diff and the backend drawing changed cells.
    pub draw_us: u64,
    /// Sending the frame to the panel (CPU time spent in flush).
    pub flush_us: u64,
    /// Whole frames, start to start.
    pub frame_us: u64,
    pub screen: &'static str,
    /// What the screen shows: the showcase's backdrop.
    pub detail: &'static str,
}

impl Stats {
    pub fn add(&mut self, other: &Stats) {
        self.frames += other.frames;
        self.update_us += other.update_us;
        self.render_us += other.render_us;
        self.draw_us += other.draw_us;
        self.flush_us += other.flush_us;
        self.frame_us += other.frame_us;
        self.screen = other.screen;
        self.detail = other.detail;
    }
}

static STATS: CriticalSectionMutex<Cell<Stats>> = CriticalSectionMutex::new(Cell::new(Stats {
    frames: 0,
    update_us: 0,
    render_us: 0,
    draw_us: 0,
    flush_us: 0,
    frame_us: 0,
    screen: "",
    detail: "",
}));
static SWITCH: AtomicBool = AtomicBool::new(false);
static PRESSES: AtomicU8 = AtomicU8::new(0);

/// Adds one frame's timings to the next report.
pub fn record(frame: &Stats) {
    STATS.lock(|s| {
        let mut total = s.get();
        total.add(frame);
        s.set(total);
    });
}

/// Whether the host asked to switch screens since the last call.
pub fn take_switch() -> bool {
    SWITCH.swap(false, Ordering::Relaxed)
}

/// Buttons the host pressed since the last call, as `input` switch bits.
pub fn take_presses() -> u8 {
    PRESSES.swap(0, Ordering::Relaxed)
}

pub fn start(usb: Peri<'static, USB>) {
    interrupt::SWI_IRQ_1.set_priority(Priority::P2);
    let spawner = EXECUTOR.start(interrupt::SWI_IRQ_1);
    // A pool of one, spawned once: this always succeeds.
    if let Ok(task) = run(usb) {
        spawner.spawn(task);
    }
}

#[embassy_executor::task]
async fn run(usb: Peri<'static, USB>) {
    let driver = Driver::new(usb, Irqs);
    let mut config = Config::new(0x2e8a, 0x0009);
    config.manufacturer = Some("rig");
    config.product = Some("badger-ratatui");
    config.serial_number = Some("badger");
    config.max_power = 250;
    config.max_packet_size_0 = 64;

    static CONFIG_DESC: StaticCell<[u8; 256]> = StaticCell::new();
    static BOS_DESC: StaticCell<[u8; 256]> = StaticCell::new();
    static CONTROL_BUF: StaticCell<[u8; 64]> = StaticCell::new();
    static STATE: StaticCell<State> = StaticCell::new();
    let mut builder = Builder::new(
        driver,
        config,
        CONFIG_DESC.init([0; 256]),
        BOS_DESC.init([0; 256]),
        &mut [],
        CONTROL_BUF.init([0; 64]),
    );
    let class = CdcAcmClass::new(&mut builder, STATE.init(State::new()), 64);
    let mut device = builder.build();
    join(device.run(), serial(class)).await;
}

async fn serial(class: CdcAcmClass<'static, Driver<'static, USB>>) {
    let (tx, rx) = class.split();
    join(report(tx), commands(rx)).await;
}

async fn report(mut tx: Sender<'static, Driver<'static, USB>>) {
    loop {
        tx.wait_connection().await;
        loop {
            Timer::after_secs(1).await;
            let s = STATS.lock(|s| s.replace(Stats::default()));
            let n = u64::from(s.frames.max(1));
            let ms = |us: u64| us as f32 / n as f32 / 1000.0;
            let mut line = String::new();
            let _ = write!(
                line,
                "{} {} fps {:.1} | frame {:.1} update {:.1} render {:.1} draw {:.1} flush {:.1} ms",
                s.screen,
                s.detail,
                s.frames as f32 * 1e6 / s.frame_us.max(1) as f32,
                ms(s.frame_us),
                ms(s.update_us),
                ms(s.render_us),
                ms(s.draw_us),
                ms(s.flush_us),
            );
            let heap = &crate::HEAP;
            let _ = write!(
                line,
                " heap {}/{} peak {} largest {}",
                heap.used(),
                heap.free(),
                heap.peak.load(Ordering::Relaxed),
                heap.largest.load(Ordering::Relaxed)
            );
            let (now, before) = crate::fault::messages();
            if let Some(fault) = now {
                let _ = write!(line, " FAULT {fault}");
            }
            if let Some(fault) = before {
                let _ = write!(line, " BEFORE RESET {fault}");
            }
            line.push('\n');
            let mut sent = Ok(());
            for chunk in line.as_bytes().chunks(64) {
                sent = tx.write_packet(chunk).await;
                if sent.is_err() {
                    break;
                }
            }
            if sent.is_err() {
                break;
            }
        }
    }
}

fn press(button: u8) {
    PRESSES.fetch_or(button, Ordering::Relaxed);
}

async fn commands(mut rx: Receiver<'static, Driver<'static, USB>>) {
    let mut line = String::new();
    let mut buf = [0u8; 64];
    loop {
        rx.wait_connection().await;
        while let Ok(n) = rx.read_packet(&mut buf).await {
            for &b in buf.iter().take(n) {
                if b == b'\n' || b == b'\r' {
                    match line.trim() {
                        "bootsel" => embassy_rp::rom_data::reset_to_usb_boot(0, 0),
                        "screen" => SWITCH.store(true, Ordering::Relaxed),
                        "press up" => press(UP),
                        "press down" => press(DOWN),
                        "press enter" => press(ENTER),
                        _ => {}
                    }
                    line.clear();
                } else if line.len() < 32 {
                    line.push(char::from(b));
                }
            }
        }
    }
}
