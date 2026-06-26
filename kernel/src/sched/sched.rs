use alloc::{
    collections::{
        btree_map::{BTreeMap, Entry},
        btree_set::BTreeSet,
        vec_deque::VecDeque,
    },
    sync::{Arc, Weak},
    vec::Vec,
};
use ksync::SpinLock;

use crate::{
    Arch,
    arch::{Architecture, ContextOf, TrapFrame, TrapFrameOf},
    percpu::{self, PerCoreContext},
    task::{self, Pid, Task, TaskState},
};

static SCHEDULER_CTX: SpinLock<GlobalScheduler> = SpinLock::new(GlobalScheduler {
    last_rq_hart_idx: 0,
    irq_wait_queue: BTreeMap::new(),
    waiting_tasks: BTreeSet::new(),
});

pub struct GlobalScheduler {
    last_rq_hart_idx: usize,
    irq_wait_queue: BTreeMap<u32, VecDeque<Weak<Task>>>,
    /// The set of tasks that are blocked on `wait`
    waiting_tasks: BTreeSet<Pid>,
}

unsafe impl Send for GlobalScheduler {}

#[repr(C)]
pub struct PerCoreScheduler {
    // TODO(aeryz): im not sure making this `Arc<Task>` right now. I want to keep this as is but I
    // also don't want to go through the runqueue and find the task and remove it when it is for
    // example killed. I guess it's no big deal to defer dropping the task until the scheduler pops
    // it since the task's resources will be freed anyways.
    /// The list of the runnable tasks for this hart.
    runqueue: VecDeque<Arc<Task>>,
    /// The list of sleeping tasks that start sleeping when it was running on
    /// this core
    sleeping_tasks: Vec<Weak<Task>>,
    /// The time when the currently running process started running.
    last_entrance_time: usize,
}

pub fn init_per_core_scheduler() -> PerCoreScheduler {
    PerCoreScheduler {
        runqueue: VecDeque::new(),
        sleeping_tasks: Vec::new(),
        last_entrance_time: 0,
    }
}

// TODO(aeryz):
// - Check if we need to set the kernel sp to 0 for the idle task
/// Schedules a task
///
/// This does not guarantee that the currently running task will change. If
/// there are no runnable tasks and the currently running task is `Ready`, it
/// will continue to run.
pub fn schedule() {
    log::trace!("Scheduling..");
    let ctx = unsafe {
        Arch::load_this_cpu_ctx::<PerCoreContext>()
            .as_mut()
            .expect("expected a valid reference to the per-CPU context")
    };

    let mut sched = ctx.scheduler.lock();
    log::trace!("sched queue len: {}", sched.runqueue.len());
    match sched.runqueue.pop_front() {
        Some(new_task) => {
            log::trace!("rq is not empty, switching to the next task");

            new_task.state.set(TaskState::Running);

            if ctx.current_task.pid != ctx.idle_task.pid
                && ctx.current_task.state == TaskState::Running
            {
                ctx.current_task.state.set(TaskState::Ready);
                sched.runqueue.push_back(Arc::clone(&ctx.current_task));
            } else if ctx.current_task.pid != ctx.idle_task.pid
                && ctx.current_task.state == TaskState::Ready
            {
                sched.runqueue.push_back(Arc::clone(&ctx.current_task));
            }

            log::trace!(
                "the new process's root page table is: 0x{:x}",
                new_task.mm.root_pt.raw()
            );

            sched.last_entrance_time = Arch::read_current_time();
            drop(sched);

            let prev_ctx =
                (&ctx.current_task.context) as *const ContextOf<Arch> as *mut ContextOf<Arch>;
            ctx.current_task = new_task;

            Arch::switch_to_user(
                prev_ctx,
                (&ctx.current_task.context) as *const ContextOf<Arch>,
                ctx.current_task.mm.root_pt.into(),
            );
        }
        None => {
            log::trace!("rq is empty, switching to the idle task");
            // If there are no tasks that we can run and the currently running task can
            // continue to be run, we just run it. This also covers if the
            // current_task is the idle task.
            if matches!(
                ctx.current_task.state.raw(),
                TaskState::Ready | TaskState::Running
            ) {
                log::trace!("current task is still ready, we don't switch to the idle task");
                // TODO(aeryz): set last entrance time??
                ctx.current_task.state.set(TaskState::Running);
                return;
            }
            log::trace!("current task is not ready, we are gonna switch to idle task");

            let prev_ctx =
                (&ctx.current_task.context) as *const ContextOf<Arch> as *mut ContextOf<Arch>;
            ctx.current_task = Arc::clone(&ctx.idle_task);
            ctx.idle_task.state.set(TaskState::Running);
            log::trace!("idle task is set to running");

            sched.last_entrance_time = Arch::read_current_time();
            drop(sched);
            Arch::set_kernel_sp(None);
            Arch::switch_to(prev_ctx, (&ctx.idle_task.context) as *const ContextOf<Arch>);
        }
    }
}

/// Enqueues a new task to one of the runqueues.
///
/// The runqueue selection is round robin as well.
pub fn enqueue_new_task(task: &Arc<Task>) {
    let idx = {
        let mut scheduler_ctx = SCHEDULER_CTX.lock();

        if scheduler_ctx.last_rq_hart_idx + 1 >= percpu::get_core_count() {
            scheduler_ctx.last_rq_hart_idx = 0;
        } else {
            scheduler_ctx.last_rq_hart_idx += 1;
        }

        scheduler_ctx.last_rq_hart_idx
    };

    let core_ctx = percpu::get_core(idx);

    unsafe {
        // TODO(aeryz): smelly
        task.trap_frame
            .as_ptr_mut::<TrapFrameOf<Arch>>()
            .unwrap()
            .as_mut()
            .unwrap()
            .set_per_core_ctx(core_ctx as *const PerCoreContext as usize);
    }

    core_ctx
        .scheduler
        .lock()
        .runqueue
        .push_back(Arc::clone(task));
}

pub fn on_timer_interrupt() {
    let ctx = unsafe {
        Arch::load_this_cpu_ctx::<PerCoreContext>()
            .as_ref()
            .expect("expected a valid reference to the per-CPU context")
    };

    let mut some_task_woke_up = false;
    let current_time = Arch::read_current_time();
    let last_entrance: usize;
    {
        let mut scheduler = ctx.scheduler.lock();
        last_entrance = scheduler.last_entrance_time;

        let mut i = 0;
        // TODO(aeryz): This is not nice, we are removing items at arbitrary indices in
        // a very hot path
        while i < scheduler.sleeping_tasks.len() {
            match scheduler.sleeping_tasks[i].upgrade() {
                Some(task) => {
                    let wake_up_at = task.runtime.lock().wake_up_at;
                    if current_time >= wake_up_at {
                        log::trace!(
                            "task should wake up at: {} and the cur is {current_time}, so we switch",
                            wake_up_at
                        );
                        task.state.set(TaskState::Ready);
                        some_task_woke_up = true;
                        let _ = scheduler.sleeping_tasks.remove(i);
                        scheduler.runqueue.push_back(task);
                    } else {
                        i += 1;
                        continue;
                    }
                }
                None => {
                    core::hint::cold_path();
                    let _ = scheduler.sleeping_tasks.remove(i);
                }
            }
        }
    }

    let set_timer = || {
        let current_time = Arch::read_current_time();
        Arch::set_timer(current_time + Arch::nanos_to_ticks(8 * 1_000_000));
    };

    if (ctx.current_task.pid != ctx.idle_task.pid
        && current_time - last_entrance > Arch::nanos_to_ticks(32 * 1_000_000))
        || some_task_woke_up
        || (ctx.current_task.pid == ctx.idle_task.pid && !ctx.scheduler.lock().runqueue.is_empty())
    {
        set_timer();
        schedule();
    } else if some_task_woke_up {
        set_timer();
        schedule();
    } else {
        set_timer();
    }
}

/// Non timer-interrupts
pub fn on_external_irq(irq: u32) {
    let task = {
        let mut scheduler_ctx = SCHEDULER_CTX.lock();
        let Some(queue) = scheduler_ctx.irq_wait_queue.get_mut(&irq) else {
            return;
        };

        let Some(task) = queue.pop_front() else {
            return;
        };

        let Some(task) = task.upgrade() else { return };

        task
    };

    task.state.set(TaskState::Ready);
    enqueue_new_task(&task);
}

/// Yields execution when a task gets blocked because of an external irq
pub fn block_on_external_irq(irq: u32) {
    let ctx = unsafe {
        Arch::load_this_cpu_ctx::<PerCoreContext>()
            .as_mut()
            .expect("expected a valid reference to the per-CPU context")
    };

    ctx.current_task.state.set(TaskState::Blocked);

    match SCHEDULER_CTX.lock().irq_wait_queue.entry(irq) {
        Entry::Vacant(e) => {
            let mut queue = VecDeque::with_capacity(1);
            queue.push_back(Arc::downgrade(&ctx.current_task));
            e.insert(queue);
        }
        Entry::Occupied(mut e) => e.get_mut().push_back(Arc::downgrade(&ctx.current_task)),
    }

    schedule();
}

pub fn block_on_wait(task: &Arc<Task>, should_block: impl FnOnce() -> bool) -> bool {
    log::trace!("blocking on wait");
    let mut scheduler = SCHEDULER_CTX.lock();

    if !should_block() {
        return false;
    }

    scheduler.waiting_tasks.insert(task.pid);
    drop(scheduler);
    schedule();
    true
}

pub fn on_task_exit(task: &Arc<Task>) {
    let parent = task.runtime.lock().parent.map(|p| task::get_task(p));
    if let Some(Some(parent)) = parent {
        if SCHEDULER_CTX.lock().waiting_tasks.remove(&parent.pid) {
            parent.state.set(TaskState::Ready);
            enqueue_new_task(&parent);
        }
    };

    schedule();
}

pub fn sleep_current_task(time_ms: usize) {
    let ctx = unsafe {
        Arch::load_this_cpu_ctx::<PerCoreContext>()
            .as_mut()
            .expect("expected a valid reference to the per-CPU context")
    };

    ctx.current_task.state.set(TaskState::Sleeping);
    // Setting an invalid time s.t. it overflows will result in this task to be
    // immediately woken up after a single time slice TODO(aeryz): check posix
    // to see how they handle overflows
    ctx.current_task.runtime.lock().wake_up_at = Arch::read_current_time()
        .checked_add(Arch::nanos_to_ticks(
            time_ms.checked_mul(1_000_000).unwrap_or(0),
        ))
        .unwrap_or(0);

    ctx.scheduler
        .lock()
        .sleeping_tasks
        .push(Arc::downgrade(&ctx.current_task));

    schedule();
}

impl core::fmt::Debug for PerCoreScheduler {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("PerCoreScheduler")
            // .field("runqueue", &self.runqueue)
            .field("last_entrance_time", &self.last_entrance_time)
            .finish()
    }
}
