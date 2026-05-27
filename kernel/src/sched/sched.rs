use core::ptr::NonNull;

use alloc::{collections::vec_deque::VecDeque, vec::Vec};
use ksync::SpinLock;

use crate::{
    Arch,
    arch::{Architecture, ContextOf, TrapFrame},
    percpu::{self, PerCoreContext},
    task::{Task, TaskState},
};

static SCHEDULER_CTX: SpinLock<GlobalScheduler> = SpinLock::new(GlobalScheduler {
    last_rq_hart_idx: 0,
});

pub struct GlobalScheduler {
    last_rq_hart_idx: usize,
}

#[repr(C)]
pub struct PerCoreScheduler {
    /// The list of the runnable tasks for this hart.
    runqueue: VecDeque<NonNull<Task>>,
    /// The list of sleeping tasks that start sleeping when it was running on
    /// this core
    sleeping_tasks: Vec<NonNull<Task>>,
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
        Some(mut task) => {
            log::trace!("rq is not empty, switching to the next task");
            let mut current_task = ctx.currently_running_task;

            let new_task = unsafe { task.as_mut() };
            new_task.state = TaskState::Running;

            ctx.currently_running_task = NonNull::new(new_task).expect("the task is nonnull");

            unsafe {
                if current_task.as_ref().state == TaskState::Ready && current_task != ctx.idle_task
                {
                    sched.runqueue.push_back(current_task);
                }
            }

            log::trace!(
                "the new process's root page table is: 0x{:x}",
                new_task.address_space.root_pt.raw()
            );

            sched.last_entrance_time = Arch::read_current_time();
            drop(sched);

            Arch::switch_to_user(
                unsafe { (&mut current_task.as_mut().context) as *mut ContextOf<Arch> },
                (&new_task.context) as *const ContextOf<Arch>,
                new_task.address_space.root_pt,
            );
        }
        None => {
            log::trace!("rq is empty, switching to the idle task");
            let current_task = unsafe { ctx.currently_running_task.as_mut() };
            // If there are no tasks that we can run and the currently running task can
            // continue to be run, we just run it. This also covers if the
            // current_task is the idle task.
            if current_task.state == TaskState::Ready {
                log::trace!("current task is still ready, we don't switch to the idle task");
                // TODO(aeryz): set last entrance time??
                current_task.state = TaskState::Running;
                return;
            }
            log::trace!("current task is not ready, we are gonna switch to idle task");

            ctx.currently_running_task = ctx.idle_task;
            let idle_task = unsafe { ctx.idle_task.as_mut() };
            idle_task.state = TaskState::Running;
            log::trace!("idle task is set to running");

            sched.last_entrance_time = Arch::read_current_time();
            drop(sched);
            Arch::set_kernel_sp(None);
            Arch::switch_to(
                (&mut current_task.context) as *mut ContextOf<Arch>,
                (&idle_task.context) as *const ContextOf<Arch>,
            );
        }
    }
}

/// Enqueues a new task to one of the runqueues.
///
/// The runqueue selection is round robin as well.
pub fn enqueue_new_task(mut task: NonNull<Task>) {
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
        (*task.as_mut().trap_frame).set_per_core_ctx(core_ctx as *const PerCoreContext as usize);
    }

    core_ctx.scheduler.lock().runqueue.push_back(task);
}

pub fn timer_interrupt() {
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
        while i < scheduler.sleeping_tasks.len() {
            let should_remove = {
                let task = unsafe { scheduler.sleeping_tasks[i].as_mut() };
                if current_time >= task.wake_up_at {
                    log::trace!(
                        "task should wake up at: {} and the cur is {current_time}, so we switch",
                        task.wake_up_at
                    );
                    task.state = TaskState::Ready;
                    some_task_woke_up = true;
                    true
                } else {
                    i += 1;
                    continue;
                }
            };

            if should_remove {
                log::trace!("time is up, putting back to the runqueue");
                let task = scheduler.sleeping_tasks.remove(i);
                scheduler.runqueue.push_back(task);
            }
        }
    }

    let set_timer = || {
        let current_time = Arch::read_current_time();
        Arch::set_timer(current_time + Arch::nanos_to_ticks(8 * 1_000_000));
    };

    if ctx.currently_running_task != ctx.idle_task
        && current_time - last_entrance > Arch::nanos_to_ticks(32 * 1_000_000)
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

pub fn sleep_current_task(time_ms: usize) {
    let ctx = unsafe {
        Arch::load_this_cpu_ctx::<PerCoreContext>()
            .as_mut()
            .expect("expected a valid reference to the per-CPU context")
    };

    let current_task = unsafe { ctx.currently_running_task.as_mut() };
    current_task.state = TaskState::Sleeping;
    // Setting an invalid time s.t. it overflows will result in this task to be
    // immediately woken up after a single time slice TODO(aeryz): check posix
    // to see how they handle overflows
    current_task.wake_up_at = Arch::read_current_time()
        .checked_add(Arch::nanos_to_ticks(
            time_ms.checked_mul(1_000_000).unwrap_or(0),
        ))
        .unwrap_or(0);

    ctx.scheduler
        .lock()
        .sleeping_tasks
        .push(ctx.currently_running_task);

    schedule();
}

impl core::fmt::Debug for PerCoreScheduler {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("PerCoreScheduler")
            .field("runqueue", &self.runqueue)
            .field("last_entrance_time", &self.last_entrance_time)
            .finish()
    }
}
