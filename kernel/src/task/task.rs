use core::ptr::NonNull;

use alloc::vec::Vec;
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
    /// Wake up time in ticks
    pub wake_up_at: usize,
    // TODO(aeryz): We can consider putting this exit code into the relevant state enum
    /// Process exit code
    pub exit_code: i32,
    /// Address space
    pub address_space: AddressSpace,
    /// List of open files
    pub file_table: SpinLock<FileTable>,
    /// Parent of this task
    pub parent: Option<Pid>,
    /// Children of this task
    pub children: Vec<Pid>,
}

pub fn create_kernel_task(entry: VirtualAddressOf<Arch>) -> NonNull<Task> {
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
        wake_up_at: 0,
        exit_code: -1,
        address_space: ADDRESS_SPACE_EMPTY,
        file_table: SpinLock::new(FileTable::init()),
        parent: None,
        children: Vec::new(),
    })
}

pub fn spawn(path: &[u8], parent: Option<NonNull<Task>>) -> Result<Pid, error::Error> {
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

    let parent = parent.map(|mut p| unsafe {
        let p = p.as_mut();
        p.children.push(pid);
        p.pid
    });

    let task_ptr = task::add_task(Task {
        pid,
        kernel_sp: VirtualAddress::from_raw(kernel_stack).expect("virtual address is valid"),
        trap_frame: trap_frame_ptr.as_ptr_mut(),
        context,
        state: TaskState::Ready.into(),
        wake_up_at: 0,
        exit_code: -1,
        address_space,
        file_table: SpinLock::new(FileTable::init()),
        parent,
        children: Vec::new(),
    });

    sched::enqueue_new_task(task_ptr);

    Ok(pid)
}

// TODO(aeryz): no sync mechanism for tasks this is scary
pub fn exit(mut task_ptr: NonNull<Task>, exit_code: i32) {
    let task = unsafe { task_ptr.as_mut() };
    if task.state == TaskState::Exited {
        return;
    }

    task.state = TaskState::Zombie.into();
    task.exit_code = exit_code;

    task.file_table.lock().destroy();
    task.address_space.free();
    task.children = Vec::new();
    // TODO(aeryz): We cannot free the kernel stack here but we need to free it
    // somewhere. The biggest problem is how we are going to free the whole task.
    // For the kernel stack at least, we can create a reaper process but I'm not
    // sure what's the best way to free the whole task yet.

    sched::on_task_exit(task_ptr);
}

pub fn wait(mut task_ptr: NonNull<Task>) -> Result<(), error::Error> {
    let task = unsafe { task_ptr.as_mut() };

    if task.children.is_empty() {
        return Ok(());
    }

    if reap_zombie_child(task) {
        return Ok(());
    }

    task.state = TaskState::Blocked.into();

    sched::block_on_wait(task_ptr);

    reap_zombie_child(task);

    Ok(())
}

fn reap_zombie_child(task: &mut Task) -> bool {
    let Some(child_idx) = task.children.iter().position(|pid| {
        let Some(child) = task::get_task(*pid) else {
            return false;
        };

        unsafe { child.as_ref().state == TaskState::Zombie }
    }) else {
        return false;
    };

    let child_pid = task.children.remove(child_idx);
    if let Some(mut child) = task::get_task(child_pid) {
        unsafe {
            child.as_mut().state = TaskState::Exited.into();
        }
    }

    true
}
