use alloc::{collections::btree_map::BTreeMap, sync::Arc, vec::Vec};
use ksync::RwLock;
use slab::Slab;

use crate::task::{Pid, Task};

const MAX_TASK_PER_SLAB: usize = 100;

static TASK_POOL: TaskPool = TaskPool(RwLock::new(TaskPoolInner {
    slabs: Vec::new(),
    pid_to_idx: BTreeMap::new(),
}));

/// A task pool that guarantees to have stable pointers to tasks meaning
/// regardless of how many tasks are inserted or removed, all of the existing
/// tasks will stay under the same address.
///
/// The fact that this is a pool instead of a contiguous table is because we
/// don't have to preallocate a very large task array but instead, allocate
/// chunks of memory as we need.
///
/// However, we should be very careful about removing a task from the pool
/// because we hand over a raw pointers to tasks. If we remove any task while
/// it's still being referenced to from some place in the kernel, any further
/// accesses to it will be UB. Hence, keeping the accesses to tasks with raw
/// pointers should be only done in known places such as the scheduler. We can
/// still access to a task using the Pid in other places.
struct TaskPool(RwLock<TaskPoolInner>);

struct TaskPoolInner {
    slabs: Vec<Slab<Arc<Task>>>,
    pid_to_idx: BTreeMap<Pid, (usize, usize)>,
}

/// Adds a task to the task pool and returns it. It returns an owned pointer to
/// it because these tasks will mostly immediately get enqueued to be run.
pub fn add_task(task: Task) -> Arc<Task> {
    let mut pool = TASK_POOL.0.write_lock();

    let slab_idx = pool
        .slabs
        .iter()
        .position(|slab| slab.len() < slab.capacity())
        .unwrap_or_else(|| {
            pool.slabs.push(Slab::with_capacity(MAX_TASK_PER_SLAB));
            pool.slabs.len() - 1
        });

    let slab = &mut pool.slabs[slab_idx];
    debug_assert!(slab.len() < slab.capacity());

    let pid = task.pid;

    let task = Arc::new(task);
    let task_idx = slab.insert(Arc::clone(&task));
    pool.pid_to_idx.insert(pid, (slab_idx, task_idx));

    task
}

/// Returns the task with `pid` if any
// NOTE(aeryz): the reason why I return `Arc` here but not `Weak` is that I
// believe most of the time this function is going to be used for immediately
// looking up for something or modifying the task. A task being present in the
// table here already means the task exists so I don't want to do unnecessary
// upgrades.
pub fn get_task(pid: Pid) -> Option<Arc<Task>> {
    let pool = TASK_POOL.0.read_lock();

    let (slab_idx, idx) = pool.pid_to_idx.get(&pid)?.clone();

    Some(Arc::clone(pool.slabs[slab_idx].get(idx)?))
}

unsafe impl Sync for TaskPool {}
