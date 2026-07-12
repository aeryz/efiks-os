mod sched;
mod sleeping_tasks;

use alloc::sync::Arc;
pub use sched::*;

use crate::task::{self, Task};

#[inline(never)]
pub fn reaper_task_main() -> ! {
    loop {
        log::info!("checking for tasks to cleanup..");

        while let Some(t) = { load_core_ctx().reaper_task.cleanup_queue.lock().pop_front() } {
            log::info!("cleaning up: {:?}", t.pid);
            task::cleanup(t);
        }

        sched::sleep_current_task(5_000);
    }
}

pub fn enqueue_for_reaper(task: Arc<Task>) {
    load_core_ctx_mut()
        .reaper_task
        .cleanup_queue
        .lock()
        .push_back(task);
}
