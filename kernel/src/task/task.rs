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
    error, exec,
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

pub fn spawn(path: &[u8]) -> Result<(), error::Error> {
    let address_space = AddressSpace::new_user();

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

    let task_ptr = task::add_task(Task {
        pid: Pid::create_next(),
        kernel_sp: VirtualAddress::from_raw(kernel_stack).expect("virtual address is valid"),
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
        ProgramHeader::validate_entsize(elf_header.class, elf_header.e_phentsize as usize)
            .map_err(|_| VfsError::Fs)?;
    let phnum = elf_header.e_phnum as usize;
    let phoff = elf_header.e_phoff as usize;
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
