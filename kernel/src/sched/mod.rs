mod sched;

pub use sched::*;

use crate::task;

#[inline(never)]
pub fn reaper_task_main() -> ! {
    loop {
        log::info!("checking for tasks to cleanup..");
        sched::sleep_current_task(5_000);

        let mut queue = load_core_ctx().reaper_task.cleanup_queue.lock();
        while let Some(t) = queue.pop_front() {
            log::info!("cleaning up: {:?}", t.pid);
            task::cleanup(t);
        }
    }
}
