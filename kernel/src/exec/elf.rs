use core::ptr;

use alloc::{
    collections::btree_map::{BTreeMap, Entry},
    vec::Vec,
};
use elf::{
    abi::{self, ET_EXEC, PT_LOAD},
    endian::LittleEndian,
    file::FileHeader,
    parse::ParseAt,
    segment::ProgramHeader,
};
use vfs::SeekFrom;

use crate::{
    arch::mmu::PteFlags,
    error,
    helper::align_down,
    mm::{self, KernelPtr, MemoryManager, PAGE_SIZE, VirtAddr},
};

#[derive(Debug)]
pub enum Error {
    Unsupported,
    SizeOverflow,
    NoProgramHeadersFound,
    InvalidProgramHeader,
    InvalidVirtualAddress,
    Parse(elf::ParseError),
}

pub struct Elf {
    file: vfs::File,
    // TODO(aeryz): Should this be AnyEndian?
    header: FileHeader<LittleEndian>,
    program_headers: Vec<ProgramHeader>,
}

/// Loads the ELF executable at `path` into the `address_space`
///
/// Returns the entrypoint address
pub fn load_executable(path: &[u8], mm_: &mut MemoryManager) -> Result<VirtAddr, error::Error> {
    let mut loader = Elf::load_from_file(path)?;

    let mut mapped_pages = BTreeMap::new();
    let mut file_page_buf = Vec::new();
    file_page_buf.resize(PAGE_SIZE, 0);

    let mut max_aligned_vaddr_end = VirtAddr::ZERO;

    for ph in loader.program_headers {
        // We only load the segments that are loadable
        if ph.p_type != PT_LOAD {
            continue;
        }

        if (ph.p_offset as usize)
            .checked_add(ph.p_filesz as usize)
            .ok_or(Error::SizeOverflow)?
            > loader.file.inode.sz()
        {
            return Err(Error::InvalidProgramHeader.into());
        }

        let mut aligned_vaddr = VirtAddr::new(ph.p_vaddr as usize).align_down(PAGE_SIZE);
        let aligned_vaddr_end = VirtAddr::new(ph.p_vaddr as usize)
            .offset_by(ph.p_memsz as isize)
            .ok_or(error::Error::Todo)?
            .align_up(PAGE_SIZE);
        max_aligned_vaddr_end = core::cmp::max(max_aligned_vaddr_end, aligned_vaddr_end);

        let flags = convert_elf_flag_to_pte(ph.p_flags);

        while aligned_vaddr < aligned_vaddr_end {
            let va = aligned_vaddr;
            match mapped_pages.entry(aligned_vaddr) {
                Entry::Vacant(e) => {
                    let pa = mm_.map_allocate_page(aligned_vaddr, flags)?;

                    let kernel_vaddr =
                        KernelPtr::<u8>::new(VirtAddr::new(mm::phys_to_virt(pa.raw())))?;

                    unsafe {
                        ptr::write_bytes(kernel_vaddr.as_ptr_mut(), 0, PAGE_SIZE);
                    }
                    e.insert(flags);
                }
                Entry::Occupied(mut e) => {
                    let new_flags = (*e.get()) | flags;
                    mm_.remap_page(va, new_flags);
                    e.insert(new_flags);
                }
            }

            aligned_vaddr = aligned_vaddr
                .offset_by(PAGE_SIZE as isize)
                .ok_or(error::Error::Todo)?;
        }

        // Based on the segment's size on the disc, we copy it to the `vaddr` that we
        // previously mapped in chunks.
        let mut copied = 0;
        while copied < ph.p_filesz as usize {
            let copy_vaddr = (ph.p_vaddr as usize)
                .checked_add(copied)
                .ok_or(Error::SizeOverflow)?;
            let page_va = align_down(copy_vaddr, PAGE_SIZE);
            let page_offset = copy_vaddr - page_va;

            // Get the previously mapped physical address from the page table so that
            // we can actually write to it. Because this `page_va` lives under the page
            // table of `address_space`. We need to convert it to kernel's mapped address.
            let page_pa = mm_
                .translate(VirtAddr::new(page_va))
                .expect("this is already mapped");
            let copy_len = (PAGE_SIZE - page_offset).min(ph.p_filesz as usize - copied);
            read_exact_at(
                &mut loader.file,
                ph.p_offset as usize + copied,
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

    mm_.set_initial_brk(max_aligned_vaddr_end);

    Ok(VirtAddr::new(loader.header.e_entry as usize))
}

impl Elf {
    pub fn load_from_file(path: &[u8]) -> Result<Elf, error::Error> {
        let mut file = crate::vfs::open(path)?;

        let header = Self::read_elf_header(&mut file)?;

        // TODO(aeryz): only support executables right now
        if header.e_type != ET_EXEC {
            return Err(Error::Unsupported.into());
        }

        // TODO(aeryz): verify target machine architecture
        // if header.e_machine

        let program_headers = Self::read_program_headers(&mut file, &header)?;

        Ok(Elf {
            file,
            header,
            program_headers,
        })
    }

    fn read_elf_header(file: &mut vfs::File) -> Result<FileHeader<LittleEndian>, error::Error> {
        let mut ident_buf = [0; abi::EI_NIDENT];
        read_exact_at(file, 0, &mut ident_buf)?;

        let ident =
            elf::file::parse_ident::<LittleEndian>(&ident_buf).map_err(|_| vfs::VfsError::Fs)?;
        let mut tail_buf = [0; elf::file::ELF64_EHDR_TAILSIZE];
        read_exact_at(file, abi::EI_NIDENT, &mut tail_buf)?;

        Ok(FileHeader::parse_tail(ident, &tail_buf).map_err(|_| vfs::VfsError::Fs)?)
    }

    fn read_program_headers(
        file: &mut vfs::File,
        header: &FileHeader<LittleEndian>,
    ) -> Result<Vec<ProgramHeader>, error::Error> {
        // if the header offset is 0 or the number of headers is 0, we don't have any
        // headers
        if header.e_phoff == 0 || header.e_phnum == 0 {
            // We are only loading executables, hence we need to have a program header
            return Err(Error::NoProgramHeadersFound.into());
        }

        // If the number of program headers are larger than 0xffff, this value is set to
        // 0xffff and we have to use another logic to load the number of headers. We
        // don't support it right now.
        if header.e_phnum == abi::PN_XNUM {
            return Err(Error::Unsupported.into());
        }

        // `e_phentsize` defines the size of a single ph entry and all entries have the
        // same size.
        let ph_size = header
            .e_phentsize
            .checked_mul(header.e_phnum)
            .ok_or(Error::SizeOverflow)?;

        let mut ph_buf = Vec::new();
        ph_buf.resize(ph_size as usize, 0u8);
        read_exact_at(file, header.e_phoff as usize, &mut ph_buf)?;

        let mut phdrs = Vec::new();
        for i in 0..header.e_phnum {
            let header = ProgramHeader::parse_at(
                header.endianness,
                header.class,
                &mut ((i * header.e_phentsize) as usize),
                &ph_buf,
            )
            .map_err(|e| Error::Parse(e))?;

            // An executable's disk size should not be larger than the memory size
            // TODO(aeryz): On which cases this might be true?
            if header.p_filesz > header.p_memsz {
                return Err(Error::InvalidProgramHeader.into());
            }

            phdrs.push(header);
        }

        Ok(phdrs)
    }
}

// TODO(aeryz): Should this really be here?
fn read_exact_at(exec: &mut vfs::File, offset: usize, buf: &mut [u8]) -> Result<(), error::Error> {
    exec.seek(SeekFrom::Start(offset))?;

    let mut n_read = 0;
    while n_read < buf.len() {
        let read = exec.read(&mut buf[n_read..])?;
        if read == 0 {
            return Err(vfs::VfsError::Fs.into());
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
