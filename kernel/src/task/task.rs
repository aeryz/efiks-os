use core::ptr::{self, NonNull};

use alloc::{collections::BTreeSet, vec::Vec};
use elf::{
    abi,
    endian::LittleEndian,
    file::{Class, FileHeader},
    parse::ParseAt,
    segment::ProgramHeader,
};
use vfs::{SeekFrom, VfsError, VfsResult};

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

pub fn spawn(path: &[u8]) -> VfsResult<()> {
    let mut exec = crate::vfs::open(path)?;
    let fsize = exec.inode.sz();

    let process_root_table_pa = mm::alloc_frame().unwrap();
    let process_root_table_va =
        VirtualAddress::from_raw(process_root_table_pa.raw() + KERNEL_DIRECT_MAPPING_BASE.raw())
            .unwrap();
    let process_root_table = process_root_table_va.as_ptr_mut();
    unsafe { *process_root_table = PageTable::empty() };
    let mut address_space = ADDRESS_SPACE_EMPTY;
    address_space.root_pt = process_root_table_pa;

    log::info!("root pt: 0x{:x}", process_root_table_pa.raw());

    let elf_header = read_elf_header(&mut exec)?;
    let segments = read_program_headers(&mut exec, fsize, &elf_header)?;
    let mut mapped_pages = BTreeSet::<usize>::new();
    let mut file_page_buf = Vec::new();
    file_page_buf.resize(PAGE_SIZE, 0);

    for seg in segments {
        if seg.p_type != elf::abi::PT_LOAD {
            continue;
        }
        log::info!(
            "Segment with the offset: {} and size {}, should be loaded at 0x{:x}.",
            seg.p_offset,
            seg.p_filesz,
            seg.p_vaddr
        );

        if seg.p_filesz > seg.p_memsz {
            return Err(VfsError::Fs);
        }

        let vaddr = usize::try_from(seg.p_vaddr).map_err(|_| VfsError::Fs)?;
        let memsz = usize::try_from(seg.p_memsz).map_err(|_| VfsError::Fs)?;
        let filesz = usize::try_from(seg.p_filesz).map_err(|_| VfsError::Fs)?;
        let file_offset = usize::try_from(seg.p_offset).map_err(|_| VfsError::Fs)?;
        let file_end = file_offset.checked_add(filesz).ok_or(VfsError::Fs)?;
        if file_end > fsize {
            return Err(VfsError::Fs);
        }

        let mem_end = vaddr + memsz;
        let map_start = align_down(vaddr, PAGE_SIZE);
        let map_end = align_up(mem_end, PAGE_SIZE);
        let flags = convert_elf_flag_to_pte(seg.p_flags);

        let mut page = map_start;
        while page < map_end {
            let va = VirtualAddress::from_raw(page).map_err(|_| VfsError::Fs)?;

            if mapped_pages.contains(&va.raw()) {
                let pa = unsafe { (*process_root_table).translate(va) }.ok_or(VfsError::Fs)?;
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
                    end: VirtualAddress::from_raw(va.raw() + PAGE_SIZE)
                        .map_err(|_| VfsError::Fs)?,
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
                    .translate(VirtualAddress::from_raw(page_va).map_err(|_| VfsError::Fs)?)
                    .ok_or(VfsError::Fs)?
            };
            read_exact_at(
                &mut exec,
                file_offset + copied,
                &mut file_page_buf[..copy_len],
            )?;

            unsafe {
                ptr::copy_nonoverlapping(
                    file_page_buf.as_ptr(),
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
            VirtualAddress::from_raw(
                usize::try_from(elf_header.e_entry).map_err(|_| VfsError::Fs)?,
            )
            .map_err(|_| VfsError::Fs)?,
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

fn read_elf_header(exec: &mut vfs::File) -> VfsResult<FileHeader<LittleEndian>> {
    let mut ident_buf = [0; abi::EI_NIDENT];
    read_exact_at(exec, 0, &mut ident_buf)?;

    let ident = elf::file::parse_ident::<LittleEndian>(&ident_buf).map_err(|_| VfsError::Fs)?;
    let tail_size = match ident.1 {
        Class::ELF32 => elf::file::ELF32_EHDR_TAILSIZE,
        Class::ELF64 => elf::file::ELF64_EHDR_TAILSIZE,
    };
    let mut tail_buf = Vec::new();
    tail_buf.resize(tail_size, 0);
    read_exact_at(exec, abi::EI_NIDENT, &mut tail_buf)?;

    FileHeader::parse_tail(ident, &tail_buf).map_err(|_| VfsError::Fs)
}

fn read_program_headers(
    exec: &mut vfs::File,
    file_size: usize,
    elf_header: &FileHeader<LittleEndian>,
) -> VfsResult<Vec<ProgramHeader>> {
    if elf_header.e_phoff == 0 || elf_header.e_phnum == 0 {
        return Ok(Vec::new());
    }

    if elf_header.e_phnum == abi::PN_XNUM {
        return Err(VfsError::Fs);
    }

    let entsize =
        ProgramHeader::validate_entsize(elf_header.class, usize::from(elf_header.e_phentsize))
            .map_err(|_| VfsError::Fs)?;
    let phnum = usize::from(elf_header.e_phnum);
    let phoff = usize::try_from(elf_header.e_phoff).map_err(|_| VfsError::Fs)?;
    let phdrs_size = entsize.checked_mul(phnum).ok_or(VfsError::Fs)?;
    let phdrs_end = phoff.checked_add(phdrs_size).ok_or(VfsError::Fs)?;
    if phdrs_end > file_size {
        return Err(VfsError::Fs);
    }

    let mut phdrs_buf = Vec::new();
    phdrs_buf.resize(phdrs_size, 0);
    read_exact_at(exec, phoff, &mut phdrs_buf)?;

    let mut phdrs = Vec::new();
    for i in 0..phnum {
        let mut offset = i.checked_mul(entsize).ok_or(VfsError::Fs)?;
        let phdr = ProgramHeader::parse_at(
            elf_header.endianness,
            elf_header.class,
            &mut offset,
            &phdrs_buf,
        )
        .map_err(|_| VfsError::Fs)?;
        let _ = phdrs.push(phdr);
    }

    Ok(phdrs)
}

fn read_exact_at(exec: &mut vfs::File, offset: usize, buf: &mut [u8]) -> VfsResult<()> {
    exec.seek(SeekFrom::Start(offset))?;

    let mut n_read = 0;
    while n_read < buf.len() {
        let read = exec.read(&mut buf[n_read..])?;
        if read == 0 {
            return Err(VfsError::Fs);
        }
        n_read += read;
    }

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
