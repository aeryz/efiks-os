use core::cell::OnceCell;

use alloc::{sync::Arc, vec::Vec};
use ksync::SpinLock;

use crate::{sched::PerCoreScheduler, task::Task};

static CORES: CoreTable = CoreTable(OnceCell::new());

#[repr(C)]
pub struct PerCoreContext {
    pub core_id: usize,
    pub scheduler: SpinLock<PerCoreScheduler>,
    pub current_task: Arc<Task>,
    pub idle_task: Arc<Task>,
    // pub reaper_task: NonNull<Task>,
}

// TODO(aeryz): Storing pointers (that can possibly be *mut) in the
// PerCpuContext unimpls the `Send` which results in the fact that we cannot put
// it in a `SpinLock`. The fact that we can have mutable reference to the
// `PerCpuContext` and `CORES` at the same time is very dangerous. This is
// probably not the correct abstraction to go. And we really need to think more
// about this.
unsafe impl Send for PerCoreContext {}

struct CoreTable(OnceCell<Vec<PerCoreContext>>);

/// SAFETY:
/// CoreTable is a fixed table that will be initialized with the cores once and
/// won't be mutated anymore.
unsafe impl Send for CoreTable {}
unsafe impl Sync for CoreTable {}

pub fn set_core_ctxs(ctx: Vec<PerCoreContext>) {
    CORES.0.get_or_init(|| ctx);
}

pub fn get_core_count() -> usize {
    CORES.0.get().expect("already initialized").len()
}

pub fn get_core<'a>(idx: usize) -> &'a PerCoreContext {
    &CORES.0.get().expect("already initialized")[idx]
}

impl core::fmt::Debug for PerCoreContext {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("PerCoreContext")
            .field("core_id", &self.core_id)
            .field("scheduler", &*self.scheduler.lock())
            // .field("currently_running_task", &self.currently_running_task)
            // .field("idle_task", &self.idle_task)
            .finish()
    }
}
