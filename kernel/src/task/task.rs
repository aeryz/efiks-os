use alloc::{sync::Arc, vec::Vec};
use ksync::SpinLock;

use crate::{
    Arch,
    arch::{Architecture, Context, ContextOf, TrapFrame, TrapFrameOf},
    error::{self, Error},
    exec,
    mm::{self, KernelVirtAddr, MemoryManager, PAGE_SIZE, VirtAddr},
    sched,
    task::{self, AtomicTaskState, Pid, TaskState, file_table::FileTable},
};

#[repr(C)]
pub struct Task {
    /// Process ID
    pub pid: Pid,
    pub trap_frame: KernelVirtAddr,
    /// Pointer to the context
    pub context: ContextOf<Arch>,
    /// The current state of the process
    pub state: AtomicTaskState,
    pub mm: MemoryManager,
    /// List of open files
    pub file_table: SpinLock<FileTable>,
    pub runtime: SpinLock<TaskRuntime>,
}

#[repr(C)]
pub struct TaskRuntime {
    /// Parent of this task
    pub parent: Option<Pid>,
    /// Children of this task
    pub children: Vec<Pid>,
    /// Process exit code
    pub exit_code: i32,
    // TODO(aeryz): this might go into sched state as well
    /// Wake up time in ticks
    pub wake_up_at: usize,
}

pub fn create_kernel_task(entry: VirtAddr) -> Result<Arc<Task>, Error> {
    let kernel_stack = KernelVirtAddr::new(mm::phys_to_virt(mm::alloc_frame()?.raw()))?;

    let context = ContextOf::<Arch>::initialize(
        entry,
        kernel_stack.offset_by(0xfa0).ok_or(Error::Todo)?.into(),
    );

    Ok(task::add_task(Task {
        pid: Pid::create_next(),
        trap_frame: KernelVirtAddr::NULL,
        context,
        state: TaskState::Ready.into(),
        mm: MemoryManager::EMPTY,
        file_table: SpinLock::new(FileTable::init()),
        runtime: SpinLock::new(TaskRuntime {
            parent: None,
            children: Vec::new(),
            exit_code: -1,
            wake_up_at: 0,
        }),
    }))
}

// TODO(aeryz): I think we should use a CStr instead since argv here doesn't
// tell you its supposed to be null-terminated right away.
pub fn spawn(path: &[u8], argv: &[&[u8]], parent: Option<&Arc<Task>>) -> Result<Pid, Error> {
    let mut mm_ = MemoryManager::new_user();

    let entry_va = exec::elf::load_executable(path, &mut mm_)?;

    let user_sp = mm_.create_user_stack()?;
    let user_sp = create_initial_stack(&mm_, user_sp, argv)?;

    let kernel_stack = mm::phys_to_virt(mm_.create_kernel_stack()?.raw());

    let trap_frame = KernelVirtAddr::new(kernel_stack - size_of::<TrapFrameOf<Arch>>())?;

    unsafe {
        *(trap_frame.as_ptr_mut()?) = TrapFrameOf::<Arch>::initialize(entry_va, user_sp);
    }

    let context = ContextOf::<Arch>::initialize(
        Arch::trap_resume_ptr().into(),
        VirtAddr::new(kernel_stack - size_of::<TrapFrameOf<Arch>>()),
    );

    let pid = Pid::create_next();

    let parent = parent.map(|p| {
        p.runtime.lock().children.push(pid);
        p.pid
    });

    let task = task::add_task(Task {
        pid,
        trap_frame,
        context,
        state: TaskState::Ready.into(),
        mm: mm_,
        file_table: SpinLock::new(FileTable::init()),
        runtime: SpinLock::new(TaskRuntime {
            parent,
            children: Vec::new(),
            exit_code: -1,
            wake_up_at: 0,
        }),
    });

    sched::enqueue_new_task(&task);

    Ok(pid)
}

// TODO(aeryz): no sync mechanism for tasks this is scary
pub fn exit(task: &Arc<Task>, exit_code: i32) {
    if task.state == TaskState::Exited {
        return;
    }

    task.state.set(TaskState::Zombie);
    task.runtime.lock().exit_code = exit_code;

    task.file_table.lock().destroy();
    task.runtime.lock().children = Vec::new();
    // The task is still running on its own page table here. Free the address
    // space from a reaper after this task is no longer current on any hart.
    // TODO(aeryz): We cannot free the kernel stack here but we need to free it
    // somewhere. The biggest problem is how we are going to free the whole task.
    // For the kernel stack at least, we can create a reaper process but I'm not
    // sure what's the best way to free the whole task yet.

    sched::on_task_exit(task);
}

pub fn wait(task: &Arc<Task>) -> Result<(), error::Error> {
    if task.runtime.lock().children.is_empty() {
        return Ok(());
    }

    task.state.set(TaskState::Blocked);

    if !sched::block_on_wait(task, || !reap_zombie_child(task)) {
        task.state.set(TaskState::Running);
        return Ok(());
    }

    reap_zombie_child(task);

    Ok(())
}

fn reap_zombie_child(task: &Arc<Task>) -> bool {
    let Some(child_idx) = task.runtime.lock().children.iter().position(|pid| {
        let Some(child) = task::get_task(*pid) else {
            return false;
        };

        child.state == TaskState::Zombie
    }) else {
        return false;
    };

    let child_pid = task.runtime.lock().children.remove(child_idx);
    if let Some(child) = task::get_task(child_pid) {
        child.state.set(TaskState::Exited);
    }

    true
}

/// Creates an initial stack for the tasks that contains the following
/// ```text
/// High addressess
/// +----------------+
/// | argv strings   |
/// +----------------+
/// | NULL           |
/// +----------------+
/// | argv[N]        |
/// +----------------+
/// | ...            |
/// +----------------+
/// | argv[0]        |
/// +----------------+
/// | argc           |
/// +----------------+ -> sp
/// Low addresses
/// ```
/// We are actually reserving enough space for the arguments in the stack.
fn create_initial_stack(
    mm_: &MemoryManager,
    user_sp: VirtAddr,
    argv: &[&[u8]],
) -> Result<VirtAddr, Error> {
    // TODO(aeryz): this assumes everything fits in a single page.
    let stack_top = user_sp;
    let stack_page_start = stack_top.align_down(PAGE_SIZE);
    let stack_top_kernel = mm_
        .translate_to_kernel(user_sp)
        .expect("created by the kernel")
        .offset_by(stack_top.difference(stack_page_start))
        .ok_or(Error::Todo)?;

    let strings_len = argv.iter().map(|arg| arg.len() + 1).sum::<usize>();
    let stack_len = strings_len + (1 + argv.len() + 1) * size_of::<usize>();
    assert!(stack_len <= stack_top.difference(stack_page_start) as usize);

    let final_sp = (stack_top.offset_by(stack_len as isize).ok_or(Error::Todo)?).align_down(0xf);
    assert!(final_sp >= stack_page_start);
    let mut argv_user_ptrs = Vec::new();

    let mut string_cursor = stack_top;
    for arg in argv.iter().rev() {
        string_cursor = string_cursor
            .offset_by(-((arg.len() + 1) as isize))
            .ok_or(Error::Todo)?;
        let string_ptr = stack_top_kernel
            .offset_by(string_cursor.difference(stack_top))
            .ok_or(Error::Todo)?
            .as_ptr_mut()?;
        unsafe {
            core::ptr::copy_nonoverlapping((*arg).as_ptr(), string_ptr, arg.len());
            *string_ptr = 0;
        }

        argv_user_ptrs.push(string_cursor);
    }
    argv_user_ptrs.reverse();

    let frame_ptr = stack_top_kernel
        .offset_by(final_sp.difference(stack_top))
        .ok_or(Error::Todo)?
        .as_ptr_mut()?;

    unsafe {
        *frame_ptr = argv.len();
        for (i, arg_ptr) in argv_user_ptrs.iter().enumerate() {
            *frame_ptr.add(1 + i) = arg_ptr.raw();
        }
        *frame_ptr.add(1 + argv.len()) = 0;
    }

    Ok(final_sp)
}
