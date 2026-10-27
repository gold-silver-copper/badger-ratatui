//! Ratatui on the GitHub Badger 2350 (Universe 2026 badge).

#![no_std]
#![no_main]

extern crate alloc;

mod cores;
mod fault;
mod lcd;
mod marble;
mod sensors;
mod usb;

use badger_ratatui::{input, showcase};

use badger_ratatui::term::{LcdBackend, TOKYO_NIGHT, WORDS};
use core::alloc::{GlobalAlloc, Layout};
use core::sync::atomic::{AtomicUsize, Ordering};

use embassy_executor::Spawner;
use embassy_rp::clocks::ClockConfig;
use embassy_rp::gpio::{Input, Level, Output, Pull};
use embassy_rp::i2c::{self, I2c};
use embassy_time::{Duration, Instant, Ticker};
use embedded_alloc::LlffHeap as Heap;
use ratatui::Terminal;
use static_cell::ConstStaticCell;

use crate::cores::TwoCores;
use crate::lcd::{Lcd, Pins};
use crate::marble::Marble;
use crate::sensors::Sensors;
use crate::showcase::Showcase;

#[global_allocator]
/// Only Ratatui allocates: its buffers, a diff per frame and the glyph cache.
/// The showcase's tables are static, so first fit doesn't fragment.
pub static HEAP: Tracked = Tracked {
    heap: Heap::empty(),
    used: AtomicUsize::new(0),
    peak: AtomicUsize::new(0),
    largest: AtomicUsize::new(0),
};
const HEAP_SIZE: usize = 128 * 1024;

/// The heap, recording its high-water mark and largest request for the USB report.
pub struct Tracked {
    heap: Heap,
    used: AtomicUsize,
    pub peak: AtomicUsize,
    pub largest: AtomicUsize,
}

impl Tracked {
    pub fn used(&self) -> usize {
        self.heap.used()
    }

    pub fn free(&self) -> usize {
        self.heap.free()
    }
}

// SAFETY: forwards to the heap; the counters only observe.
unsafe impl GlobalAlloc for Tracked {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        self.largest.fetch_max(layout.size(), Ordering::Relaxed);
        // SAFETY: the caller's contract, passed on.
        let ptr = unsafe { self.heap.alloc(layout) };
        if !ptr.is_null() {
            let used = self.used.fetch_add(layout.size(), Ordering::Relaxed) + layout.size();
            self.peak.fetch_max(used, Ordering::Relaxed);
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        self.used.fetch_sub(layout.size(), Ordering::Relaxed);
        // SAFETY: the caller's contract, passed on.
        unsafe { self.heap.dealloc(ptr, layout) }
    }
}

/// The panel's pixels, in its byte order: 150 KiB.
static FRAMEBUFFER: ConstStaticCell<[u32; WORDS]> = ConstStaticCell::new([0; WORDS]);

/// The showcase's per-sample tables, too big for the stack.
static SHOWCASE_BUFFERS: ConstStaticCell<showcase::Buffers> =
    ConstStaticCell::new(showcase::Buffers::new());

#[unsafe(link_section = ".bi_entries")]
#[used]
pub static PICOTOOL_ENTRIES: [embassy_rp::binary_info::EntryAddr; 3] = [
    embassy_rp::binary_info::rp_program_name!(c"badger-ratatui"),
    embassy_rp::binary_info::rp_program_description!(c"Ratatui on the GitHub Badger 2350"),
    embassy_rp::binary_info::rp_program_build_attribute!(),
];

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    // SAFETY: called once, before anything allocates.
    unsafe {
        let heap = &HEAP.heap;
        embedded_alloc::init!(heap, HEAP_SIZE);
    }

    // 200 MHz, which Raspberry Pi validates for the RP2350 at its default core
    // voltage (150 MHz is the reset default).
    let clocks =
        ClockConfig::system_freq(200_000_000).unwrap_or_else(|_| ClockConfig::crystal(12_000_000));
    fault::init();
    let p = embassy_rp::init(embassy_rp::config::Config::new(clocks));
    usb::start(p.USB);
    let mut cores = TwoCores::start(p.CORE1);

    // Switched 3.3 V rail: the IMU, touch controller and I2C pull-ups.
    let _power = Output::new(p.PIN_41, Level::High);

    let lcd = Lcd::new(
        Pins {
            backlight: p.PIN_26,
            cs: p.PIN_27,
            rs: p.PIN_28,
            wr: p.PIN_30,
            rd: p.PIN_31,
            db: (
                p.PIN_32, p.PIN_33, p.PIN_34, p.PIN_35, p.PIN_36, p.PIN_37, p.PIN_38, p.PIN_39,
            ),
        },
        p.PIO1,
        p.DMA_CH0,
    );

    let mut i2c_config = i2c::Config::default();
    i2c_config.frequency = 400_000;
    let i2c = I2c::new_blocking(p.I2C1, p.PIN_19, p.PIN_18, i2c_config);
    let switches = [
        Input::new(p.PIN_6, Pull::Up),  // down
        Input::new(p.PIN_7, Pull::Up),  // left (SW_A)
        Input::new(p.PIN_9, Pull::Up),  // right (SW_B)
        Input::new(p.PIN_10, Pull::Up), // enter (SW_C)
        Input::new(p.PIN_11, Pull::Up), // up
    ];
    let mut sensors = Sensors::new(i2c, switches);
    // The case LEDs, held off.
    let _leds = [
        Output::new(p.PIN_0, Level::Low),
        Output::new(p.PIN_1, Level::Low),
        Output::new(p.PIN_2, Level::Low),
        Output::new(p.PIN_3, Level::Low),
    ];

    let backend = LcdBackend::new(lcd, FRAMEBUFFER.take(), TOKYO_NIGHT);
    let Ok(mut terminal) = Terminal::new(backend);
    // The backlight comes on once the cleared screen is on the panel.
    let _ = terminal.flush();
    terminal.backend_mut().panel_mut().light();

    let mut marble = Marble::new();
    let mut showcase = Showcase::new(0x2350, SHOWCASE_BUFFERS.take());
    let mut show_demo = true;
    let mut previous_switches = 0u8;
    let mut nav_held = 0u8;
    let mut nav_since = Instant::now();
    let mut ticker = Ticker::every(Duration::from_hz(60));
    let mut last = Instant::now();
    let mut bootsel_held = Instant::now();
    loop {
        let now = Instant::now();
        let dt = (now - last).as_micros() as f32 / 1e6;
        let mut stats = usb::Stats {
            frames: 1,
            frame_us: (now - last).as_micros(),
            ..usb::Stats::default()
        };
        last = now;

        let mut input = sensors.read();
        // Held for this one frame, which the showcase sees as a press.
        input.switches |= usb::take_presses();
        if usb::take_switch() {
            show_demo = !show_demo;
        }
        // Hold physical up + down for two seconds to reboot into the USB bootloader.
        let up_down = sensors::UP | sensors::DOWN;
        if input.switches & up_down != up_down {
            bootsel_held = now;
        } else if now - bootsel_held > Duration::from_secs(2) {
            embassy_rp::rom_data::reset_to_usb_boot(0, 0);
        }

        // Debounce navigation and switch once per press, not every frame held.
        let navigation = input.switches & (sensors::LEFT | sensors::RIGHT);
        if navigation != nav_held {
            nav_held = navigation;
            nav_since = now;
        }
        if now - nav_since >= Duration::from_millis(30) {
            let pressed = navigation & !previous_switches;
            previous_switches = navigation;
            if pressed == sensors::LEFT || pressed == sensors::RIGHT {
                show_demo = !show_demo;
            }
        }

        if show_demo {
            showcase.update(&input, dt);
        } else {
            marble.update(input, dt.min(0.1), now.as_millis());
        }
        let drawing = Instant::now();
        stats.update_us = (drawing - now).as_micros();
        let mut render_us = 0;
        let _ = terminal.draw(|f| {
            let start = Instant::now();
            if show_demo {
                showcase.render(f.area(), f.buffer_mut(), &mut cores);
            } else {
                marble.draw(f);
            }
            render_us = start.elapsed().as_micros();
        });
        stats.render_us = render_us;
        stats.flush_us = lcd::take_bus_us();
        stats.draw_us = drawing
            .elapsed()
            .as_micros()
            .saturating_sub(render_us + stats.flush_us);
        (stats.screen, stats.detail) = if show_demo {
            ("showcase", showcase.backdrop())
        } else {
            ("marble", "")
        };
        usb::record(&stats);
        ticker.next().await;
    }
}
