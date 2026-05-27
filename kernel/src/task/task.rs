use core::ptr::{self, NonNull};

use alloc::{collections::BTreeSet, vec::Vec};
use elf::endian::LittleEndian;

use crate::{
    Arch,
    arch::{
        Architecture, Context, ContextOf, TrapFrame, TrapFrameOf, VirtualAddressOf,
        mmu::{PageTable, PhysicalAddress, PteFlags, VirtualAddress},
    },
    helper::{align_down, align_up},
    mm::{self, KERNEL_DIRECT_MAPPING_BASE},
    sched,
    task::{self, ADDRESS_SPACE_EMPTY, AddressSpace, Pid, TaskState, VmRegion},
};

// TODO(aeryz): this is still a bogus address and can collide with the other
// stuff. Have a task address limit and reserve the stack near the top. And
// check for collisions.
pub const TASK_STACK_ADDRESS: VirtualAddress =
    unsafe { VirtualAddress::from_raw_unchecked(0x0000_0000_3fff_3fa0) };

const PAGE_SIZE: usize = 4096;

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

    log::info!("root pt: 0x{:x}", process_root_table_pa.raw());

    let elf_bytes = elf::ElfBytes::<LittleEndian>::minimal_parse(&buf).unwrap();
    let mut mapped_pages = BTreeSet::<usize>::new();

    for seg in elf_bytes.segments().unwrap() {
        if seg.p_type != elf::abi::PT_LOAD {
            continue;
        }
        log::info!(
            "Segment with the offset: {} and size {}, should be loaded at 0x{:x}.",
            seg.p_offset,
            seg.p_filesz,
            seg.p_vaddr
        );

        let vaddr = seg.p_vaddr as usize;
        let memsz = seg.p_memsz as usize;
        let filesz = seg.p_filesz as usize;
        let map_start = align_down(vaddr, PAGE_SIZE);
        let map_end = align_up(vaddr + memsz, PAGE_SIZE);
        let flags = convert_elf_flag_to_pte(seg.p_flags);

        let mut page = map_start;
        while page < map_end {
            let va = VirtualAddress::from_raw(page).unwrap();

            if mapped_pages.contains(&va.raw()) {
                let pa = unsafe { (*process_root_table).translate(va) }.unwrap();
                unsafe {
                    (*process_root_table).map_vm(va, pa, flags);
                }
            } else {
                let pa = mm::alloc_frame().unwrap();

                unsafe {
                    ptr::write_bytes(mm::phys_to_virt(pa.raw()) as *mut u8, 0, PAGE_SIZE);
                    log::info!("mapping 0x{:x} to 0x{:x}", va.raw(), pa.raw());
                    (*process_root_table).map_vm(va, pa, flags);
                }

                let _ = mapped_pages.insert(va.raw());
                let _ = address_space.regions.push(VmRegion {
                    start: va,
                    end: VirtualAddress::from_raw(va.raw() + PAGE_SIZE).unwrap(),
                });
            }

            page += PAGE_SIZE;
        }

        let mut copied = 0;
        while copied < filesz {
            let copy_vaddr = vaddr + copied;
            let page_va = align_down(copy_vaddr, PAGE_SIZE);
            let page_offset = copy_vaddr - page_va;
            let copy_len = (PAGE_SIZE - page_offset).min(filesz - copied);
            let page_pa = unsafe {
                (*process_root_table)
                    .translate(VirtualAddress::from_raw(page_va).unwrap())
                    .unwrap()
            };

            unsafe {
                ptr::copy_nonoverlapping(
                    buf.as_ptr().add(seg.p_offset as usize + copied),
                    (mm::phys_to_virt(page_pa.raw()) + page_offset) as *mut u8,
                    copy_len,
                );
            }

            copied += copy_len;
        }
    }

    // TODO(aeryz): these address space reserve stuff and the elf loading above
    // overloads this function too much. These responsibilities needs to be
    // separated.
    for i in 0..4 {
        let user_stack = mm::alloc_frame().unwrap();

        let va = VirtualAddress::from_raw(0x0000_0000_3fff_0000 + 0x1000 * i).unwrap();
        unsafe { (*process_root_table).map_vm(va, user_stack, PteFlags::RW | PteFlags::U) };
        let _ = address_space.regions.push(VmRegion {
            start: va,
            end: VirtualAddress::from_raw(va.raw() + 4096).unwrap(),
        });
    }

    let mut kernel_stack_bottom = PhysicalAddress::ZERO;
    for _ in 0..4 {
        // TODO(aeryz): With the following logic, we cannot guarantee a 16KB contiguous
        // virtual memory. This is not acceptable.
        let kernel_stack = mm::alloc_frame().unwrap();
        let kernel_stack_va =
            VirtualAddress::from_raw(mm::phys_to_virt(kernel_stack.raw())).unwrap();
        let _ = address_space.regions.push(VmRegion {
            start: kernel_stack_va,
            end: VirtualAddress::from_raw(kernel_stack_va.raw() + 4096).unwrap(),
        });
        unsafe {
            (*process_root_table).map_vm(kernel_stack_va, kernel_stack, PteFlags::RW);
        }

        kernel_stack_bottom = PhysicalAddress::from_raw(kernel_stack.raw() + 0xfa0).unwrap();
    }

    let kernel_stack_bottom = mm::phys_to_virt(kernel_stack_bottom.raw());

    let trap_frame_ptr =
        VirtualAddress::from_raw(kernel_stack_bottom - size_of::<TrapFrameOf<Arch>>()).unwrap();

    unsafe {
        *(trap_frame_ptr.as_ptr_mut()) = TrapFrameOf::<Arch>::initialize(
            VirtualAddress::from_raw(elf_bytes.ehdr.e_entry as usize).unwrap(),
            TASK_STACK_ADDRESS,
        );
    }

    let context = ContextOf::<Arch>::initialize(
        Arch::trap_resume_ptr(),
        VirtualAddress::from_raw(kernel_stack_bottom - size_of::<TrapFrameOf<Arch>>()).unwrap(),
    );

    mm::kvm_full_map(unsafe { process_root_table.as_mut().unwrap() });

    let task_ptr = task::add_task(Task {
        pid: Pid::create_next(),
        kernel_sp: VirtualAddress::from_raw(kernel_stack_bottom).expect("virtual address is valid"),
        trap_frame: trap_frame_ptr.as_ptr_mut(),
        context,
        state: TaskState::Ready,
        wake_up_at: 0,
        exit_code: -1,
        address_space,
    });

    sched::enqueue_new_task(task_ptr);

    Ok(())
}

fn convert_elf_flag_to_pte(elf_flag: u32) -> PteFlags {
    let mut flags = PteFlags::U;

    if (elf_flag & elf::abi::PF_R) != 0 {
        flags |= PteFlags::R;
    }

    if (elf_flag & elf::abi::PF_W) != 0 {
        flags |= PteFlags::W;
    }

    if (elf_flag & elf::abi::PF_X) != 0 {
        flags |= PteFlags::X;
    }

    flags
}
