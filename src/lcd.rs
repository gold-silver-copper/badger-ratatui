//! ST7789 320x240 panel on the badge's 8-bit parallel (8080) bus.
//!
//! PIO1 drives the bus: data DB0..DB7 on GPIO32..39 and the write strobe on
//! GPIO30, through the PIO's upper GPIO window (GPIOBASE 16). Commands are
//! pushed by the CPU; frames go out by DMA straight from the framebuffer
//! while the CPU computes the next one.

use core::ops::Range;
use core::sync::atomic::{AtomicU32, Ordering, compiler_fence};

use badger_ratatui::term::{self, Panel, WORDS};
use embassy_rp::Peri;
use embassy_rp::bind_interrupts;
use embassy_rp::clocks::clk_sys_freq;
use embassy_rp::gpio::{Level, Output};
use embassy_rp::pac;
use embassy_rp::pac::dma::vals::{DataSize, TreqSel};
use embassy_rp::peripherals::{
    DMA_CH0, PIN_26, PIN_27, PIN_28, PIN_30, PIN_31, PIN_32, PIN_33, PIN_34, PIN_35, PIN_36,
    PIN_37, PIN_38, PIN_39, PIO1,
};
use embassy_rp::pio::{
    Common, Config, Direction, FifoJoin, InterruptHandler, Pin, Pio, ShiftConfig, ShiftDirection,
    StateMachine,
};
use embassy_time::{Duration, Instant, block_for};

bind_interrupts!(struct Irqs {
    PIO1_IRQ_0 => InterruptHandler<PIO1>;
});

/// Memory access control: row/column order and XY swap give the badge's
/// upright landscape view.
const MADCTL: u8 = 0xB0;
/// The DMA channel `DMA_CH0` claims.
const DMA: usize = 0;
/// The ST7789 needs a write cycle of at least 66 ns; a byte takes four PIO
/// cycles, so PIO clocks of up to 50 MHz give 80 ns.
const PIO_HZ: u32 = 50_000_000;

pub struct Pins {
    pub backlight: Peri<'static, PIN_26>,
    pub cs: Peri<'static, PIN_27>,
    pub rs: Peri<'static, PIN_28>,
    pub wr: Peri<'static, PIN_30>,
    pub rd: Peri<'static, PIN_31>,
    pub db: DataPins,
}

/// DB0..DB7.
pub type DataPins = (
    Peri<'static, PIN_32>,
    Peri<'static, PIN_33>,
    Peri<'static, PIN_34>,
    Peri<'static, PIN_35>,
    Peri<'static, PIN_36>,
    Peri<'static, PIN_37>,
    Peri<'static, PIN_38>,
    Peri<'static, PIN_39>,
);

pub struct Lcd {
    rs: Output<'static>,
    backlight: Output<'static>,
    sm: StateMachine<'static, PIO1, 0>,
    // Held for as long as the bus runs.
    _cs: Output<'static>,
    _rd: Output<'static>,
    _pio: Common<'static, PIO1>,
    _pins: [Pin<'static, PIO1>; 9],
    _dma: Peri<'static, DMA_CH0>,
}

impl Lcd {
    pub fn new(p: Pins, pio: Peri<'static, PIO1>, dma: Peri<'static, DMA_CH0>) -> Self {
        let Pio {
            mut common,
            mut sm0,
            ..
        } = Pio::new(pio, Irqs);
        let program = embassy_rp::pio::program::pio_asm!(
            ".side_set 1",
            ".wrap_target",
            "    out pins, 8   side 0 [1]",
            "    nop           side 1 [1]",
            ".wrap",
        );
        let (d0, d1, d2, d3, d4, d5, d6, d7) = p.db;
        let wr = common.make_pio_pin(p.wr);
        let db = [
            common.make_pio_pin(d0),
            common.make_pio_pin(d1),
            common.make_pio_pin(d2),
            common.make_pio_pin(d3),
            common.make_pio_pin(d4),
            common.make_pio_pin(d5),
            common.make_pio_pin(d6),
            common.make_pio_pin(d7),
        ];
        let mut config = Config::default();
        config.use_program(&common.load_program(&program.program), &[&wr]);
        config.set_out_pins(&db.each_ref());
        config.shift_out = ShiftConfig {
            threshold: 8,
            direction: ShiftDirection::Right,
            auto_fill: true,
        };
        config.fifo_join = FifoJoin::TxOnly;
        let divider = clk_sys_freq().div_ceil(PIO_HZ).max(1);
        config.clock_divider = u16::try_from(divider).unwrap_or(u16::MAX).into();
        sm0.set_config(&config);
        sm0.set_pins(Level::High, &[&wr]);
        sm0.set_pin_dirs(Direction::Out, &[&wr]);
        sm0.set_pin_dirs(Direction::Out, &db.each_ref());
        sm0.set_enable(true);

        let [d0, d1, d2, d3, d4, d5, d6, d7] = db;
        let mut lcd = Self {
            rs: Output::new(p.rs, Level::High),
            backlight: Output::new(p.backlight, Level::Low),
            sm: sm0,
            // The panel is alone on the bus: selected for good.
            _cs: Output::new(p.cs, Level::Low),
            _rd: Output::new(p.rd, Level::High),
            _pio: common,
            _pins: [wr, d0, d1, d2, d3, d4, d5, d6, d7],
            _dma: dma,
        };
        lcd.init();
        lcd
    }

    /// Pimoroni's init sequence for their 320x240 ST7789 panels.
    fn init(&mut self) {
        self.command(0x01, &[]); // SWRESET
        block_for(Duration::from_millis(150));
        self.command(0x35, &[]); // TEON
        self.command(0x3A, &[0x05]); // COLMOD: 16 bpp
        self.command(0xB2, &[0x0c, 0x0c, 0x00, 0x33, 0x33]); // PORCTRL
        self.command(0xC0, &[0x2c]); // LCMCTRL
        self.command(0xC2, &[0x01]); // VDVVRHEN
        self.command(0xC3, &[0x12]); // VRHS
        self.command(0xC4, &[0x20]); // VDVS
        self.command(0xD0, &[0xa4, 0xa1]); // PWCTRL1
        self.command(0xC6, &[0x0f]); // FRCTRL2: 60 Hz
        self.command(0xB0, &[0x00, 0xc0]); // RAMCTRL: big-endian pixels
        self.command(0xB7, &[0x35]); // GCTRL
        self.command(0xBB, &[0x1f]); // VCOMS
        self.command(
            0xE0,
            &[
                0xD0, 0x08, 0x11, 0x08, 0x0C, 0x15, 0x39, 0x33, 0x50, 0x36, 0x13, 0x14, 0x29, 0x2D,
            ],
        );
        self.command(
            0xE1,
            &[
                0xD0, 0x08, 0x10, 0x08, 0x06, 0x06, 0x39, 0x44, 0x51, 0x0B, 0x16, 0x14, 0x2F, 0x31,
            ],
        );
        self.command(0x21, &[]); // INVON
        self.command(0x11, &[]); // SLPOUT
        block_for(Duration::from_millis(10));
        self.command(0x36, &[MADCTL]);
        self.command(0x29, &[]); // DISPON
    }

    /// The backlight, once the first frame is on the panel.
    pub fn light(&mut self) {
        self.idle();
        self.backlight.set_high();
    }

    fn push(&mut self, byte: u8) {
        while !self.sm.tx().try_push(u32::from(byte)) {}
    }

    /// Waits until the last byte has been strobed out.
    fn idle(&mut self) {
        while dma_busy() {}
        while !self.sm.tx().empty() {}
        // The stall flag is sticky: clear it, then see it set again by the
        // state machine stalling on the empty FIFO after its last byte.
        let _ = self.sm.tx().stalled();
        while !self.sm.tx().stalled() {}
    }

    fn command(&mut self, cmd: u8, data: &[u8]) {
        self.idle();
        self.rs.set_low();
        self.push(cmd);
        self.idle();
        self.rs.set_high();
        for &b in data {
            self.push(b);
        }
    }
}

fn dma_busy() -> bool {
    pac::DMA.ch(DMA).ctrl_trig().read().busy()
}

impl Panel for Lcd {
    fn wait(&mut self) {
        let start = Instant::now();
        while dma_busy() {}
        compiler_fence(Ordering::SeqCst);
        BUS_US.fetch_add(start.elapsed().as_micros() as u32, Ordering::Relaxed);
    }

    fn send(&mut self, framebuffer: &[u32; WORDS], rows: Range<usize>) {
        let start = Instant::now();
        let (Ok(y0), Ok(y1)) = (u16::try_from(rows.start), u16::try_from(rows.end)) else {
            return;
        };
        let Some(bytes) = framebuffer
            .get(rows.start * term::WIDTH / 2..rows.end * term::WIDTH / 2)
            .filter(|words| !words.is_empty())
        else {
            return;
        };
        let [y0h, y0l] = y0.to_be_bytes();
        let [y1h, y1l] = (y1 - 1).to_be_bytes();
        let [x1h, x1l] = (term::WIDTH as u16 - 1).to_be_bytes();
        self.command(0x2A, &[0, 0, x1h, x1l]); // CASET
        self.command(0x2B, &[y0h, y0l, y1h, y1l]); // RASET
        self.command(0x2C, &[]); // RAMWR: the pixels follow
        // The framebuffer's writes land before the DMA reads it.
        compiler_fence(Ordering::SeqCst);
        let ch = pac::DMA.ch(DMA);
        ch.read_addr().write_value(bytes.as_ptr() as u32);
        ch.write_addr()
            .write_value(pac::PIO1.txf(0).as_ptr() as u32);
        ch.trans_count().write(|w| {
            w.set_mode(0.into());
            w.set_count((bytes.len() * 4) as u32);
        });
        ch.ctrl_trig().write(|w| {
            w.set_treq_sel(TreqSel::PIO1_TX0);
            w.set_data_size(DataSize::SIZE_BYTE);
            w.set_incr_read(true);
            w.set_incr_write(false);
            w.set_chain_to(DMA as u8);
            w.set_en(true);
        });
        BUS_US.fetch_add(start.elapsed().as_micros() as u32, Ordering::Relaxed);
    }
}

/// CPU microseconds spent waiting on or starting transfers since the last
/// `take_bus_us`.
static BUS_US: AtomicU32 = AtomicU32::new(0);

pub fn take_bus_us() -> u64 {
    u64::from(BUS_US.swap(0, Ordering::Relaxed))
}
