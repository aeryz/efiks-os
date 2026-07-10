mod addr;
mod frame_allocator;
mod kernel_allocator;
mod kvm;
mod mappings;

use core::ptr;

pub use addr::*;
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
const KERNEL_STACK_PAGES: usize = 16;
const KERNEL_STACK_RESERVED_BYTES: usize = 0x60;

#[allow(unused)]
#[derive(Clone)]
pub struct VmRegion {
    /// Start address of this region
    pub start: VirtAddr,
    /// End address of this region
    pub end: VirtAddr,
    /// Permissions
    // TODO(aeryz): pteflags here is again arch specific
    pub flags: PteFlags,
}

#[derive(Copy, Clone)]
pub struct KernelStackRegion {
    pub guard: PhysAddr,
    pub start: VirtAddr,
    pub end: VirtAddr,
}

impl KernelStackRegion {
    pub const EMPTY: Self = Self {
        guard: PhysAddr::ZERO,
        start: VirtAddr::ZERO,
        end: VirtAddr::ZERO,
    };
}

pub struct MemoryManager {
    pub root_pt: PhysAddr,
    /// **Sorted** mapped regions
    pub regions: SpinLock<Vec<VmRegion>>,
    pub kernel_stack: SpinLock<KernelStackRegion>,
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
        kernel_stack: SpinLock::new(KernelStackRegion::EMPTY),
        start_brk: VirtAddr::ZERO,
        brk: SpinLock::new(VirtAddr::ZERO),
    };

    /// Creates a new address space.
    pub fn new() -> Self {
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
        // Keep one allocated frame below the stack. With the current direct-map
        // setup this is not an unmapped guard page, but it prevents a shallow
        // kernel stack underflow from corrupting the task's root page table.
        let guard = alloc_frame().unwrap();
        // TODO(aeryz): With the following logic, we cannot guarantee contiguous
        // virtual memory. This is not acceptable.
        let kernel_stack_start = alloc_frame().unwrap();
        for _ in 1..KERNEL_STACK_PAGES {
            let _ = alloc_frame().unwrap();
        }

        // We don't do mapping here because we already did `kvm_full_map` which maps the
        // entire memory with 1GB pages.
        let stack_size = KERNEL_STACK_PAGES * PAGE_SIZE;
        let stack_start_va = VirtAddr::new(phys_to_virt(kernel_stack_start.raw()));
        let stack_end_va = stack_start_va
            .offset_by(stack_size as isize)
            .ok_or(Error::Overflow)?;
        let kernel_stack_top = kernel_stack_start
            .offset_by((stack_size - KERNEL_STACK_RESERVED_BYTES) as isize)
            .unwrap();

        *self.kernel_stack.lock() = KernelStackRegion {
            guard,
            start: stack_start_va,
            end: stack_end_va,
        };

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

        self.insert_mapping(
            va,
            va.offset_by(PAGE_SIZE as isize).ok_or(Error::Overflow)?,
            flags,
        )?;

        Ok(pa)
    }

    /// Handles a page fault at `addr`. Allocates if VM mapping exists,
    /// otherwise returns error.
    pub fn handle_page_fault(&self, addr: VirtAddr, access_flags: PteFlags) -> Result<(), Error> {
        let addr = addr.align_down(PAGE_SIZE);
        let regions = self.regions.lock();
        let flags = match regions.binary_search_by_key(&addr, |r| r.start) {
            Ok(i) => Ok(regions[i].flags),
            Err(i) => (i > 0 && addr < regions[i - 1].end)
                .then_some(regions[i - 1].flags)
                .ok_or(Error::Unmapped),
        }?;

        if !flags.contains(access_flags) {
            return Err(Error::Unmapped);
        }

        let pa = alloc_frame().unwrap();
        zero_frame(pa);
        MemoryModelOf::<Arch>::map_vm(self.root_pt_virt().into(), addr.into(), pa.into(), flags);
        Arch::flush_tlb();

        Ok(())
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
                        flags,
                    },
                );

                Ok(true)
            }
        }
    }

    /// Inserts a mapping in a sorted way
    fn insert_mapping(&self, start: VirtAddr, end: VirtAddr, flags: PteFlags) -> Result<(), Error> {
        let mut regions = self.regions.lock();
        match regions.binary_search_by_key(&start, |va| va.start) {
            Ok(_) => panic!("Mapping an already existing entry again must be handled."),
            Err(i) => regions.insert(i, VmRegion { start, end, flags }),
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
                // Then this is a shared kernel mapping.
                continue;
            }

            if let Some(pa) = self.translate(r.start) {
                free_frame(pa);
            }
        }
        drop(regions);

        self.free_kernel_stack();

        MemoryModelOf::<Arch>::traverse_free(self.root_pt.into());
    }

    pub fn translate_to_kernel(&self, va: VirtAddr) -> Result<VirtAddr, Error> {
        Ok(VirtAddr::new(phys_to_virt(
            self.translate(va).ok_or(Error::Unmapped)?.raw(),
        )))
    }

    pub fn translate(&self, va: VirtAddr) -> Option<PhysAddr> {
        MemoryModelOf::<Arch>::translate(self.root_pt_virt().into(), va.into()).map(Into::into)
    }

    fn root_pt_virt(&self) -> VirtAddr {
        VirtAddr::new(phys_to_virt(self.root_pt.raw()))
    }

    fn free_kernel_stack(&self) {
        let stack = *self.kernel_stack.lock();
        if stack.guard == PhysAddr::ZERO {
            debug_assert_eq!(self.root_pt, PhysAddr::ZERO);
            return;
        }

        free_frame(stack.guard);

        let mut va = stack.start;
        while va < stack.end {
            free_frame(PhysAddr::new(virt_to_phys(va.raw())));
            va = va.offset_by(PAGE_SIZE as isize).unwrap();
        }
    }
}

fn zero_frame(pa: PhysAddr) {
    unsafe {
        ptr::write_bytes(phys_to_virt(pa.raw()) as *mut u8, 0, PAGE_SIZE);
    }
}
