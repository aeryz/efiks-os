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

struct TaskPool(SpinLock<TaskPoolInner>);

struct TaskPoolInner {
    slabs: Vec<Slab<Task>>,
    pid_to_idx: BTreeMap<Pid, (usize, usize)>,
}

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
    let task_ptr =
        NonNull::new(slab.get_mut(task_idx).unwrap() as *mut Task).expect("task is nonnull");
    pool.pid_to_idx.insert(pid, (slab_idx, task_idx));

    task_ptr
}

unsafe impl Sync for TaskPool {}
