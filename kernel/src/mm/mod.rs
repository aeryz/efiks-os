mod address;
mod frame_allocator;
mod kernel_allocator;
mod kvm;
mod mappings;

use core::ptr;

pub use address::*;
use alloc::vec::Vec;
#[allow(unused)]
pub use frame_allocator::{alloc_frame, free_frame};
use ksync::SpinLock;
pub use kvm::*;
pub use mappings::*;

use crate::{
    Arch,
    arch::{Architecture, MemoryModel, MemoryModelOf, mmu::PteFlags},
    error::Error,
};

pub const PAGE_SIZE: usize = 4096;

#[allow(unused)]
#[derive(Clone)]
pub struct VmRegion {
    /// Start address of this region
    pub start: VirtAddr,
    /// End address of this region
    pub end: VirtAddr,
}

pub struct MemoryManager {
    pub root_pt: PhysAddr,
    /// **Sorted** mapped regions
    pub regions: SpinLock<Vec<VmRegion>>,
    pub start_brk: VirtAddr,
    pub brk: SpinLock<VirtAddr>,
}

// TODO(aeryz): No way of allocating and mapping large tables. Idk how this
// should be done actually. Should this be the call of the `AddressSpace` where
// for example, when loading a single (2MB + 10 4KB) page, should this
// dynamically figure that it should reserve 1 2MB and 10 4KB pages?
impl MemoryManager {
    pub const EMPTY: Self = Self {
        root_pt: PhysAddr::ZERO,
        regions: SpinLock::new(Vec::new()),
        start_brk: VirtAddr::ZERO,
        brk: SpinLock::new(VirtAddr::ZERO),
    };

    /// Creates a new address space for the user tasks
    pub fn new_user() -> Self {
        let root_pt = alloc_frame().unwrap();

        let mut self_ = Self::EMPTY;
        self_.root_pt = root_pt;

        let root_pt = VirtAddr::new(phys_to_virt(root_pt.raw()));

        MemoryModelOf::<Arch>::initialize_empty_pt(root_pt.into());

        kvm_full_map(root_pt);

        self_
    }

    pub fn brk(&self, new_brk: VirtAddr) -> Result<VirtAddr, Error> {
        let mut brk = self.brk.lock();
        if self.start_brk >= new_brk {
            return Ok(*brk);
        }

        if new_brk < *brk {
            // TODO(aeryz): i don't think immediately freeing the region makes sense but idk
            // what's the best thing to do here.
            // we don't immediately free the region for now.
            *brk = new_brk;
            Ok(*brk)
        } else if new_brk > *brk {
            let mut addr = brk.align_up(PAGE_SIZE);
            let new_mapped_end = new_brk.align_up(PAGE_SIZE);
            let mut mapped_new_page = false;

            while addr < new_mapped_end {
                mapped_new_page |=
                    self.map_allocate_page_if_not_exist(addr, PteFlags::RW | PteFlags::U)?;
                addr = addr.offset_by(PAGE_SIZE as isize).ok_or(Error::Overflow)?;
            }

            if mapped_new_page {
                Arch::flush_tlb();
            }

            *brk = new_brk;
            Ok(*brk)
        } else {
            Ok(*brk)
        }
    }

    pub fn create_user_stack(&self) -> Result<VirtAddr, Error> {
        // 32KB user stack
        let mut va = VirtAddr::new(0x0000_0000_3fff_0000);
        for _ in 0..8 {
            va = va.offset_by(PAGE_SIZE as isize).ok_or(Error::Overflow)?;

            self.map_allocate_page(va, PteFlags::RW | PteFlags::U)?;
        }

        // TODO(aeryz): why -0x60 here and how's this gonna effect the alignment
        va.offset_by(PAGE_SIZE as isize - 0x60)
            .ok_or(Error::Overflow)
    }

    pub fn create_kernel_stack(&self) -> Result<PhysAddr, Error> {
        let mut kernel_stack_top = PhysAddr::ZERO;
        // 32KB kernel stack
        for _ in 0..8 {
            // TODO(aeryz): With the following logic, we cannot guarantee a 16KB contiguous
            // virtual memory. This is not acceptable.
            let kernel_stack = alloc_frame().unwrap();
            let kernel_stack_va = VirtAddr::new(phys_to_virt(kernel_stack.raw()));
            self.insert_mapping(
                kernel_stack_va,
                kernel_stack_va
                    .offset_by(PAGE_SIZE as isize)
                    .ok_or(Error::Overflow)?,
            )?;

            // We don't do mapping here because we already did `kvm_full_map` which maps the
            // entire memory with 1GB pages

            kernel_stack_top = kernel_stack.offset_by(0xfa0).unwrap();
        }

        Ok(kernel_stack_top)
    }

    pub fn set_initial_brk(&mut self, brk: VirtAddr) {
        self.start_brk = brk;
        *self.brk.lock() = brk;
    }

    /// Allocates a single 4k physical page, maps `va` to it and adds it to the
    /// regions.
    pub fn map_allocate_page(&self, va: VirtAddr, flags: PteFlags) -> Result<PhysAddr, Error> {
        let pa = alloc_frame().unwrap();

        MemoryModelOf::<Arch>::map_vm(self.root_pt_virt().into(), va.into(), pa.into(), flags);

        self.insert_mapping(va, va.offset_by(PAGE_SIZE as isize).ok_or(Error::Overflow)?)?;

        Ok(pa)
    }

    fn map_allocate_page_if_not_exist(
        &self,
        addr: VirtAddr,
        flags: PteFlags,
    ) -> Result<bool, Error> {
        let mut regions = self.regions.lock();
        match regions.binary_search_by_key(&addr, |r| r.start) {
            // it could match exactly
            Ok(_) => Ok(false),
            // or it would not and then binary search will give us the location on where would it be
            // inserted to be sorted. With this, we know that `addr` is smaller than the start
            // address of the region at `i`. So we check whether we are within the region at `i -
            // 1`.
            Err(i) => {
                if i > 0 && addr < regions[i - 1].end {
                    return Ok(false);
                }

                let pa = alloc_frame().unwrap();
                zero_frame(pa);
                MemoryModelOf::<Arch>::map_vm(
                    self.root_pt_virt().into(),
                    addr.into(),
                    pa.into(),
                    flags,
                );

                regions.insert(
                    i,
                    VmRegion {
                        start: addr,
                        end: addr.offset_by(PAGE_SIZE as isize).ok_or(Error::Overflow)?,
                    },
                );

                Ok(true)
            }
        }
    }

    /// Inserts a mapping in a sorted way
    fn insert_mapping(&self, start: VirtAddr, end: VirtAddr) -> Result<(), Error> {
        let mut regions = self.regions.lock();
        match regions.binary_search_by_key(&start, |va| va.start) {
            Ok(_) => return Err(Error::Todo),
            Err(i) => regions.insert(i, VmRegion { start, end }),
        }

        Ok(())
    }

    /// Remap a previously mapped page. This is used for overriding the flags
    /// basically.
    pub fn remap_page(&mut self, va: VirtAddr, flags: PteFlags) {
        MemoryModelOf::<Arch>::remap_vm(self.root_pt_virt().into(), va.into(), flags);
    }

    /// Frees up all the memory and removes all the internal refs.
    pub fn free(&self) {
        let regions = self.regions.lock();
        for r in regions.iter() {
            if r.start > KERNEL_DIRECT_MAPPING_BASE {
                // Then this is a kernel mapping. We cannot free the kernel
                // stack at this point since it is still in-use.
                continue;
            }
            let pa = self.translate(r.start).unwrap();
            free_frame(pa);
        }

        MemoryModelOf::<Arch>::traverse_free(self.root_pt.into());
    }

    pub fn translate_to_kernel(&self, va: VirtAddr) -> Result<KernelVirtAddr, Error> {
        KernelVirtAddr::new(phys_to_virt(self.translate(va).ok_or(Error::Todo)?.raw()))
    }

    pub fn translate(&self, va: VirtAddr) -> Option<PhysAddr> {
        MemoryModelOf::<Arch>::translate(self.root_pt_virt().into(), va.into()).map(Into::into)
    }

    fn root_pt_virt(&self) -> VirtAddr {
        VirtAddr::new(phys_to_virt(self.root_pt.raw()))
    }
}

fn zero_frame(pa: PhysAddr) {
    unsafe {
        ptr::write_bytes(phys_to_virt(pa.raw()) as *mut u8, 0, PAGE_SIZE);
    }
}
