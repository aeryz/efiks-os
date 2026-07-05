mod sched;

pub use sched::*;

#[inline(never)]
pub fn reaper_task_main() -> ! {
    loop {}
}
