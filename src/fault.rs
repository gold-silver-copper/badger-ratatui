//! Records a panic or hard fault, on either core, so the USB report can say
//! what stopped the firmware.
//!
//! A panic in thread mode parks its core; the USB port, run from an
//! interrupt, keeps reporting. A hard fault, or a panic in an interrupt,
//! would block USB too, so it saves its message in RAM that a reset leaves
//! alone and resets: the next boot reports it.

use core::fmt::Write;
use core::mem::MaybeUninit;
use core::panic::PanicInfo;
use core::sync::atomic::{AtomicBool, Ordering};

use cortex_m_rt::{ExceptionFrame, exception};

const MAGIC: u32 = 0xfa17_ed00;
const SIZE: usize = 160;

struct Saved {
    magic: u32,
    len: usize,
    bytes: [u8; SIZE],
}

/// Survives a reset: `.uninit` is neither zeroed nor loaded at boot.
#[unsafe(link_section = ".uninit.fault")]
static mut SAVED: MaybeUninit<Saved> = MaybeUninit::uninit();

static TAKEN: AtomicBool = AtomicBool::new(false);
static READY: AtomicBool = AtomicBool::new(false);
static mut NOW: [u8; SIZE] = [0; SIZE];
static mut NOW_LEN: usize = 0;
static mut BEFORE: [u8; SIZE] = [0; SIZE];
static mut BEFORE_LEN: usize = 0;

/// At boot: takes a fault the last run saved before it reset.
pub fn init() {
    // SAFETY: called once at boot on core 0, before anything else reads these.
    unsafe {
        let saved = (&raw mut SAVED).cast::<Saved>();
        if (*saved).magic == MAGIC {
            (&raw mut BEFORE).write((*saved).bytes);
            BEFORE_LEN = (*saved).len.min(SIZE);
        }
        (*saved).magic = 0;
    }
}

/// This run's fault, and one the last run reset after, if any.
pub fn messages() -> (Option<&'static str>, Option<&'static str>) {
    // SAFETY: NOW is written once, before READY is set; BEFORE only in `init`.
    unsafe {
        let now = READY
            .load(Ordering::Acquire)
            .then(|| {
                (&raw const NOW)
                    .as_ref()
                    .and_then(|b| b.get(..NOW_LEN))
                    .and_then(|b| core::str::from_utf8(b).ok())
            })
            .flatten();
        let before = (&raw const BEFORE)
            .as_ref()
            .and_then(|b| b.get(..BEFORE_LEN))
            .filter(|b| !b.is_empty());
        (now, before.and_then(|b| core::str::from_utf8(b).ok()))
    }
}

struct Buf<'a>(&'a mut [u8; SIZE], usize);

impl Write for Buf<'_> {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        for &b in s.as_bytes() {
            if let Some(slot) = self.0.get_mut(self.1) {
                *slot = b;
                self.1 += 1;
            }
        }
        Ok(())
    }
}

/// Writes the first fault's message; later ones are dropped.
fn record(args: core::fmt::Arguments) -> bool {
    if TAKEN.swap(true, Ordering::AcqRel) {
        return false;
    }
    let core = embassy_rp::pac::SIO.cpuid().read();
    // SAFETY: only the one caller that won TAKEN writes here.
    unsafe {
        if let Some(now) = (&raw mut NOW).as_mut() {
            let mut buf = Buf(now, 0);
            let _ = write!(buf, "core {core}: {args}");
            NOW_LEN = buf.1;
        }
        let saved = (&raw mut SAVED).cast::<Saved>();
        (*saved).bytes = (&raw const NOW).read();
        (*saved).len = NOW_LEN;
        (*saved).magic = MAGIC;
    }
    READY.store(true, Ordering::Release);
    true
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    match info.location() {
        Some(l) => record(format_args!(
            "panic at {}:{}: {}",
            l.file(),
            l.line(),
            info.message()
        )),
        None => record(format_args!("panic: {}", info.message())),
    };
    // In an interrupt, parking would block USB too: reset, and report then.
    if cortex_m::peripheral::SCB::vect_active() != cortex_m::peripheral::scb::VectActive::ThreadMode
    {
        cortex_m::peripheral::SCB::sys_reset();
    }
    loop {
        cortex_m::asm::wfe();
    }
}

#[exception]
unsafe fn HardFault(frame: &ExceptionFrame) -> ! {
    record(format_args!(
        "hard fault at pc {:#010x} lr {:#010x}",
        frame.pc(),
        frame.lr()
    ));
    cortex_m::peripheral::SCB::sys_reset()
}
