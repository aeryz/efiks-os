use core::ptr::{self, NonNull};

use alloc::vec::Vec;
use elf::endian::LittleEndian;

use crate::{
    Arch,
    arch::{
        Architecture, Context, ContextOf, TrapFrame, TrapFrameOf, VirtualAddressOf,
        mmu::{PageTable, PhysicalAddress, PteFlags, VirtualAddress},
    },
    mm::{self, KERNEL_DIRECT_MAPPING_BASE},
    sched,
    task::{self, ADDRESS_SPACE_EMPTY, AddressSpace, Pid, TaskState, VmRegion},
};

pub const TASK_TEXT_ADDRESS: VirtualAddress =
    unsafe { VirtualAddress::from_raw_unchecked(0x1_0000) };
pub const TASK_STACK_ADDRESS: VirtualAddress =
    unsafe { VirtualAddress::from_raw_unchecked(0x0000_0000_3fff_3fa0) };

#[repr(C)]
#[derive(Clone)]
pub struct Task {
    /// Process ID
    pub pid: Pid,
    /// Kernel stack pointer
    pub kernel_sp: VirtualAddressOf<Arch>,
    pub trap_frame: *mut TrapFrameOf<Arch>,
    /// Pointer to the context
    pub context: ContextOf<Arch>,
    /// The current state of the process
    pub state: TaskState,
    /// Wake up time in ticks
    pub wake_up_at: usize,
    // TODO(aeryz): We can consider putting this exit code into the relevant state enum
    /// Process exit code
    pub exit_code: i32,
    /// Address space
    pub address_space: AddressSpace,
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
        state: TaskState::Ready,
        wake_up_at: 0,
        exit_code: -1,
        address_space: ADDRESS_SPACE_EMPTY,
    })
}

pub fn spawn(path: &[u8]) -> vfs::VfsResult<()> {
    let mut exec = crate::vfs::open(path)?;
    let fsize = exec.inode.sz();
    let mut n_read = 0;

    let mut buf = Vec::new();
    buf.resize(exec.inode.sz(), 0);

    while n_read < fsize {
        n_read += exec.read(&mut buf)?;
    }

    let process_root_table_pa = mm::alloc_frame().unwrap();
    let process_root_table_va =
        VirtualAddress::from_raw(process_root_table_pa.raw() + KERNEL_DIRECT_MAPPING_BASE.raw())
            .unwrap();
    let process_root_table = process_root_table_va.as_ptr_mut();
    unsafe { *process_root_table = PageTable::empty() };
    let mut address_space = ADDRESS_SPACE_EMPTY;
    address_space.root_pt = process_root_table_pa;

    let elf_bytes = elf::ElfBytes::<LittleEndian>::minimal_parse(&buf).unwrap();
    for seg in elf_bytes.segments().unwrap() {
        if seg.p_type != elf::abi::PT_LOAD {
            log::info!("other: {}", seg.p_type);
            continue;
        }
        log::info!(
            "Segment with the offset: {} and size {}, should be loaded at 0x{:x}.",
            seg.p_offset,
            seg.p_filesz,
            seg.p_vaddr
        );

        let va = VirtualAddress::from_raw(seg.p_vaddr as usize).unwrap();
        let pa = mm::alloc_frame().unwrap();

        unsafe {
            ptr::copy(
                &buf[seg.p_offset as usize] as *const u8,
                mm::phys_to_virt(pa.raw()) as *mut u8,
                seg.p_filesz as usize,
            );
        }

        unsafe {
            (*process_root_table).map_vm(va, pa, convert_elf_flag_to_pte(seg.p_flags));
            let _ = address_space.regions.push(VmRegion {
                start: va,
                end: VirtualAddress::from_raw(va.raw() + seg.p_filesz as usize).unwrap(),
            });
        }
    }

    Ok(())
}

pub fn create_task(entry: VirtualAddressOf<Arch>) -> NonNull<Task> {
    // we first initiate user's root page table
    let process_root_table_pa = mm::alloc_frame().unwrap();
    let process_root_table_va =
        VirtualAddress::from_raw(process_root_table_pa.raw() + KERNEL_DIRECT_MAPPING_BASE.raw())
            .unwrap();
    let process_root_table = process_root_table_va.as_ptr_mut();
    unsafe { *process_root_table = PageTable::empty() };

    let mut address_space = ADDRESS_SPACE_EMPTY;
    address_space.root_pt = process_root_table_pa;

    // we don't do heap for now
    // TODO: we temporarily load the user process from the kernel by just mapping it
    // in the userspace

    // Assuming the code is at most 64K
    for i in 0..16 {
        let va = VirtualAddress::from_raw(0x0000_0000_0001_0000 + 0x1000 * i).unwrap();
        unsafe {
            (*process_root_table).map_vm(
                va,
                PhysicalAddress::from_raw_unchecked(
                    entry.raw() - 0xffff_ffff_0000_0000 + 0x1000 * i,
                ),
                PteFlags::RX | PteFlags::U,
            );
        }
        let _ = address_space.regions.push(VmRegion {
            start: va,
            end: VirtualAddress::from_raw(va.raw() + 4096).unwrap(),
        });
    }

    // 16K stack
    for i in 0..4 {
        let user_stack = mm::alloc_frame().unwrap();

        let va = VirtualAddress::from_raw(0x0000_0000_3fff_0000 + 0x1000 * i).unwrap();
        unsafe { (*process_root_table).map_vm(va, user_stack, PteFlags::RW | PteFlags::U) };
        let _ = address_space.regions.push(VmRegion {
            start: va,
            end: VirtualAddress::from_raw(va.raw() + 4096).unwrap(),
        });
    }

    let mut kernel_stack_pa = PhysicalAddress::ZERO;
    // 16K kernel stack
    for i in 0..4 {
        let kernel_stack = mm::alloc_frame().unwrap();
        let kernel_stack_va = VirtualAddress::from_raw(0x0000_0000_4fff_0000 + 0x1000 * i).unwrap();

        let _ = address_space.regions.push(VmRegion {
            start: kernel_stack_va,
            end: VirtualAddress::from_raw(kernel_stack_va.raw() + 4096).unwrap(),
        });

        unsafe { (*process_root_table).map_vm(kernel_stack_va, kernel_stack, PteFlags::RW) };

        kernel_stack_pa = kernel_stack;
    }

    let kernel_view_of_the_users_kernel_stack =
        kernel_stack_pa.raw() + KERNEL_DIRECT_MAPPING_BASE.raw() + 0xfa0;

    let trap_frame_ptr = VirtualAddress::from_raw(
        kernel_view_of_the_users_kernel_stack - size_of::<TrapFrameOf<Arch>>(),
    )
    .unwrap();

    unsafe {
        *(trap_frame_ptr.as_ptr_mut()) =
            TrapFrameOf::<Arch>::initialize(task::TASK_TEXT_ADDRESS, task::TASK_STACK_ADDRESS);
    }

    let context = ContextOf::<Arch>::initialize(
        Arch::trap_resume_ptr(),
        VirtualAddress::from_raw(0x0000_0000_4fff_3fa0 - size_of::<TrapFrameOf<Arch>>()).unwrap(),
    );

    mm::kvm_full_map(unsafe { process_root_table.as_mut().unwrap() });

    let task_ptr = task::add_task(Task {
        pid: Pid::create_next(),
        kernel_sp: VirtualAddress::from_raw(0x0000_0000_4fff_3fa0)
            .expect("virtual address is valid"),
        trap_frame: trap_frame_ptr.as_ptr_mut(),
        context,
        state: TaskState::Ready,
        wake_up_at: 0,
        exit_code: -1,
        address_space,
    });

    sched::enqueue_new_task(task_ptr);

    task_ptr
}

fn convert_elf_flag_to_pte(elf_flag: u32) -> PteFlags {
    let mut flags = PteFlags::U;

    log::info!("converting flags");

    if (elf_flag & elf::abi::PF_R) != 0 {
        log::info!("Read");
        flags |= PteFlags::R;
    }

    if (elf_flag & elf::abi::PF_W) != 0 {
        log::info!("Write");
        flags |= PteFlags::W;
    }

    if (elf_flag & elf::abi::PF_X) != 0 {
        log::info!("Exec");
        flags |= PteFlags::X;
    }

    flags
}
