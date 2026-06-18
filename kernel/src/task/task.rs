use core::ptr::NonNull;

use alloc::{sync::Arc, vec::Vec};
use ksync::SpinLock;

use crate::{
    Arch,
    arch::{
        Architecture, Context, ContextOf, TrapFrame, TrapFrameOf, VirtualAddressOf,
        mmu::VirtualAddress,
    },
    error, exec,
    mm::{self, KERNEL_DIRECT_MAPPING_BASE},
    sched,
    task::{
        self, ADDRESS_SPACE_EMPTY, AddressSpace, AtomicTaskState, Pid, TaskState,
        file_table::FileTable,
    },
};

#[repr(C)]
pub struct Task {
    /// Process ID
    pub pid: Pid,
    /// Kernel stack pointer
    pub kernel_sp: VirtualAddressOf<Arch>,
    pub trap_frame: *mut TrapFrameOf<Arch>,
    /// Pointer to the context
    pub context: ContextOf<Arch>,
    /// The current state of the process
    pub state: AtomicTaskState,
    /// Address space
    pub address_space: AddressSpace,
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

pub fn create_kernel_task(entry: VirtualAddressOf<Arch>) -> Arc<Task> {
    let kernel_stack = mm::alloc_frame().unwrap();
    let kernel_stack_va =
        VirtualAddress::from_raw(kernel_stack.raw() + KERNEL_DIRECT_MAPPING_BASE.raw()).unwrap();

    // TODO(aeryz): I don't like this
    let kernel_sp = VirtualAddress::from_raw(kernel_stack_va.raw() + 0xfa0).unwrap();
    let context = ContextOf::<Arch>::initialize(entry, kernel_sp);

    task::add_task(Task {
        pid: Pid::create_next(),
        kernel_sp,
        trap_frame: core::ptr::null_mut(),
        context,
        state: TaskState::Ready.into(),
        address_space: ADDRESS_SPACE_EMPTY,
        file_table: SpinLock::new(FileTable::init()),
        runtime: SpinLock::new(TaskRuntime {
            parent: None,
            children: Vec::new(),
            exit_code: -1,
            wake_up_at: 0,
        }),
    })
}

pub fn spawn(path: &[u8], parent: Option<&Arc<Task>>) -> Result<Pid, error::Error> {
    let mut address_space = AddressSpace::new_user();

    let entry_va = exec::elf::load_executable(path, &mut address_space)?;

    let user_stack = address_space.create_user_stack();

    let kernel_stack = mm::phys_to_virt(address_space.create_kernel_stack().raw());

    let trap_frame_ptr =
        VirtualAddress::from_raw(kernel_stack - size_of::<TrapFrameOf<Arch>>()).unwrap();

    unsafe {
        *(trap_frame_ptr.as_ptr_mut()) = TrapFrameOf::<Arch>::initialize(entry_va, user_stack);
    }

    let context = ContextOf::<Arch>::initialize(
        Arch::trap_resume_ptr(),
        VirtualAddress::from_raw(kernel_stack - size_of::<TrapFrameOf<Arch>>()).unwrap(),
    );

    let pid = Pid::create_next();

    let parent = parent.map(|p| unsafe {
        p.runtime.lock().children.push(pid);
        p.pid
    });

    let task = task::add_task(Task {
        pid,
        kernel_sp: VirtualAddress::from_raw(kernel_stack).expect("virtual address is valid"),
        trap_frame: trap_frame_ptr.as_ptr_mut(),
        context,
        state: TaskState::Ready.into(),
        address_space,
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
