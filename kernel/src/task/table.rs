use core::ptr::NonNull;

use alloc::{collections::btree_map::BTreeMap, vec::Vec};
use ksync::SpinLock;
use slab::Slab;

use crate::task::{Pid, Task};

const MAX_TASK_PER_SLAB: usize = 100;

static TASK_POOL: TaskPool = TaskPool(SpinLock::new(TaskPoolInner {
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
struct TaskPool(SpinLock<TaskPoolInner>);

struct TaskPoolInner {
    slabs: Vec<Slab<Task>>,
    pid_to_idx: BTreeMap<Pid, (usize, usize)>,
}

/// Adds a task to the task pool and returns a pointer to it. As long as the
/// task itself is not reaped, the pointer will be valid.
pub fn add_task(task: Task) -> NonNull<Task> {
    let mut pool = TASK_POOL.0.lock();

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
    let task_idx = slab.insert(task);
    log::info!("adding task with id: {}", task_idx);
    let task_ptr =
        NonNull::new(slab.get_mut(task_idx).unwrap() as *mut Task).expect("task is nonnull");
    log::info!("added task with id: {}", task_idx);
    pool.pid_to_idx.insert(pid, (slab_idx, task_idx));

    task_ptr
}

/// Returns the task with `pid` if any
pub fn get_task(pid: Pid) -> Option<NonNull<Task>> {
    let mut pool = TASK_POOL.0.lock();

    let (slab_idx, idx) = pool.pid_to_idx.get(&pid)?.clone();

    NonNull::new(pool.slabs[slab_idx].get_mut(idx)?)
}

unsafe impl Sync for TaskPool {}
