//! Both of the RP2350's cores for the showcase's shading: core 1 waits for a
//! job, runs it while core 0 runs its own half, and says when it is done.

use core::sync::atomic::{AtomicUsize, Ordering};

use badger_ratatui::showcase::Cores;
use embassy_rp::Peri;
use embassy_rp::multicore::{Stack, spawn_core1};
use embassy_rp::peripherals::CORE1;
use static_cell::ConstStaticCell;

/// The job for core 1: the address of a `&mut (dyn FnMut() + Send)` on core
/// 0's stack, or 0 when there is none. Core 1 sets it back to 0 when done.
static JOB: AtomicUsize = AtomicUsize::new(0);
static STACK: ConstStaticCell<Stack<16384>> = ConstStaticCell::new(Stack::new());

type Job<'a> = &'a mut (dyn FnMut() + Send);

pub struct TwoCores(());

impl TwoCores {
    pub fn start(core1: Peri<'static, CORE1>) -> Self {
        spawn_core1(core1, STACK.take(), || {
            loop {
                let job = JOB.load(Ordering::Acquire);
                if job == 0 {
                    cortex_m::asm::wfe();
                    continue;
                }
                // SAFETY: core 0 stored the address of a live `Job` and does
                // not touch it, or return from `join`, until JOB is 0 again.
                let job = unsafe { &mut *(job as *mut Job) };
                job();
                JOB.store(0, Ordering::Release);
                signal();
            }
        });
        Self(())
    }
}

impl Cores for TwoCores {
    fn join(&mut self, a: &mut (dyn FnMut() + Send), b: &mut (dyn FnMut() + Send)) {
        let mut job: Job = a;
        JOB.store(&raw mut job as usize, Ordering::Release);
        signal();
        b();
        // `job` borrows `a`, which must outlive core 1's use of it: wait here.
        while JOB.load(Ordering::Acquire) != 0 {
            cortex_m::asm::wfe();
        }
    }
}

/// Wakes the other core from `wfe`. The barrier first makes the store to JOB
/// visible to it: without one the event can arrive before the store, the
/// other core sees no change and sleeps again, and core 1, which takes no
/// interrupts, then never wakes.
fn signal() {
    cortex_m::asm::dsb();
    cortex_m::asm::sev();
}
