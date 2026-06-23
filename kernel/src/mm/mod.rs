mod frame_allocator;
mod kernel_allocator;
mod kvm;
mod mappings;

use core::ptr;

use alloc::vec::Vec;
#[allow(unused)]
pub use frame_allocator::{alloc_frame, free_frame};
use ksync::SpinLock;
pub use kvm::*;
pub use mappings::*;

use crate::{
    Arch,
    arch::{
        Architecture, PhysicalAddressOf, VirtualAddressOf,
        mmu::{PageTable, PhysicalAddress, PteFlags, VirtualAddress},
    },
    helper,
};

pub const PAGE_SIZE: usize = 4096;

#[allow(unused)]
#[derive(Clone)]
pub struct VmRegion {
    /// Start address of this region
    pub start: VirtualAddressOf<Arch>,
    /// End address of this region
    pub end: VirtualAddressOf<Arch>,
}

pub struct MemoryManager {
    pub root_pt: PhysicalAddressOf<Arch>,
    /// **Sorted** mapped regions
    pub regions: SpinLock<Vec<VmRegion>>,
    pub start_brk: VirtualAddressOf<Arch>,
    pub brk: SpinLock<VirtualAddressOf<Arch>>,
}

// TODO(aeryz): No way of allocating and mapping large tables. Idk how this
// should be done actually. Should this be the call of the `AddressSpace` where
// for example, when loading a single (2MB + 10 4KB) page, should this
// dynamically figure that it should reserve 1 2MB and 10 4KB pages?
impl MemoryManager {
    pub const EMPTY: Self = Self {
        root_pt: PhysicalAddress::ZERO,
        regions: SpinLock::new(Vec::new()),
        start_brk: VirtualAddress::invalid(),
        brk: SpinLock::new(VirtualAddress::invalid()),
    };

    /// Creates a new address space for the user tasks
    pub fn new_user() -> Self {
        let root_pt = alloc_frame().unwrap();

        let mut self_ = Self::EMPTY;
        self_.root_pt = root_pt;

        unsafe {
            *(self_.root_pt_ptr()) = PageTable::empty();
            kvm_full_map(self_.root_pt_ptr().as_mut().unwrap());
        }

        self_
    }

    pub fn brk(&self, new_brk: VirtualAddressOf<Arch>) -> VirtualAddressOf<Arch> {
        if self.start_brk.raw() >= new_brk.raw() {
            return self.start_brk;
        }

        let mut brk = self.brk.lock();

        if new_brk.raw() < brk.raw() {
            // TODO(aeryz): i don't think immediately freeing the region makes sense but idk
            // what's the best thing to do here.
            // we don't immediately free the region for now.
            *brk = new_brk;
            *brk
        } else if new_brk.raw() > brk.raw() {
            let mut addr = helper::align_up(brk.raw(), PAGE_SIZE);
            let new_mapped_end = helper::align_up(new_brk.raw(), PAGE_SIZE);
            let mut mapped_new_page = false;

            while addr < new_mapped_end {
                mapped_new_page |= self.map_allocate_page_if_not_exist(
                    VirtualAddress::from_raw(addr).unwrap(),
                    PteFlags::RW | PteFlags::U,
                );
                addr += PAGE_SIZE;
            }

            if mapped_new_page {
                Arch::flush_tlb();
            }

            *brk = new_brk;
            *brk
        } else {
            *brk
        }
    }

    pub fn create_user_stack(&self) -> VirtualAddressOf<Arch> {
        // 32KB user stack
        let mut va = unsafe { VirtualAddress::from_raw_unchecked(0x0000_0000_3fff_0000) };
        for _ in 0..8 {
            va = VirtualAddress::from_raw(va.raw() + PAGE_SIZE).unwrap();

            self.map_allocate_page(va, PteFlags::RW | PteFlags::U);
        }

        VirtualAddress::from_raw(va.raw() + PAGE_SIZE - 0x60).unwrap()
    }

    pub fn create_kernel_stack(&self) -> PhysicalAddressOf<Arch> {
        let mut kernel_stack_top = PhysicalAddress::ZERO;
        // 32KB kernel stack
        for _ in 0..8 {
            // TODO(aeryz): With the following logic, we cannot guarantee a 16KB contiguous
            // virtual memory. This is not acceptable.
            let kernel_stack = alloc_frame().unwrap();
            let kernel_stack_va =
                VirtualAddress::from_raw(phys_to_virt(kernel_stack.raw())).unwrap();
            self.insert_mapping(kernel_stack_va, unsafe {
                VirtualAddress::from_raw_unchecked(kernel_stack_va.raw() + PAGE_SIZE)
            });

            // We don't do mapping here because we already did `kvm_full_map` which maps the
            // entire memory with 1GB pages

            kernel_stack_top = PhysicalAddress::from_raw(kernel_stack.raw() + 0xfa0).unwrap();
        }

        kernel_stack_top
    }

    pub fn set_initial_brk(&mut self, brk: VirtualAddressOf<Arch>) {
        log::info!("brk is 0x{:x}", brk.raw());
        self.start_brk = brk;
        *self.brk.lock() = brk;
    }

    /// Allocates a single 4k physical page, maps `va` to it and adds it to the
    /// regions.
    pub fn map_allocate_page(
        &self,
        va: VirtualAddressOf<Arch>,
        flags: PteFlags,
    ) -> PhysicalAddressOf<Arch> {
        let pa = alloc_frame().unwrap();

        unsafe {
            (*self.root_pt_ptr()).map_vm(va, pa, flags);
        };

        self.insert_mapping(va, unsafe {
            VirtualAddress::from_raw_unchecked(va.raw() + PAGE_SIZE)
        });

        pa
    }

    fn map_allocate_page_if_not_exist(
        &self,
        addr: VirtualAddressOf<Arch>,
        flags: PteFlags,
    ) -> bool {
        let mut regions = self.regions.lock();
        match regions.binary_search_by_key(&addr.raw(), |r| r.start.raw()) {
            // it could match exactly
            Ok(_) => false,
            // or it would not and then binary search will give us the location on where would it be
            // inserted to be sorted. With this, we know that `addr` is smaller than the start
            // address of the region at `i`. So we check whether we are within the region at `i -
            // 1`.
            Err(i) => {
                if i > 0 && addr.raw() < regions[i - 1].end.raw() {
                    return false;
                }

                let pa = alloc_frame().unwrap();
                zero_frame(pa);
                unsafe {
                    (*self.root_pt_ptr()).map_vm(addr, pa, flags);
                }

                regions.insert(
                    i,
                    VmRegion {
                        start: addr,
                        end: unsafe { VirtualAddress::from_raw_unchecked(addr.raw() + PAGE_SIZE) },
                    },
                );

                true
            }
        }
    }

    /// Inserts a mapping in a sorted way
    fn insert_mapping(&self, start: VirtualAddressOf<Arch>, end: VirtualAddressOf<Arch>) {
        let mut regions = self.regions.lock();
        match regions.binary_search_by_key(&start.raw(), |va| va.start.raw()) {
            Ok(_) => panic!("we should handle double mapping case"),
            Err(i) => regions.insert(i, VmRegion { start, end }),
        }
    }

    /// Remap a previously mapped page. This is used for overriding the flags
    /// basically.
    pub fn remap_page(&mut self, va: VirtualAddressOf<Arch>, flags: PteFlags) {
        unsafe {
            (*self.root_pt_ptr()).remap_vm(va, flags);
        };
    }

    /// Frees up all the memory and removes all the internal refs.
    pub fn free(&self) {
        let regions = self.regions.lock();
        for r in regions.iter() {
            if r.start.raw() > KERNEL_DIRECT_MAPPING_BASE.raw() {
                // Then this is a kernel mapping. We cannot free the kernel
                // stack at this point since it is still in-use.
                continue;
            }
            let pa = self.translate(r.start).unwrap();
            free_frame(pa);
        }

        PageTable::traverse_free(self.root_pt);
    }

    pub fn translate(&self, va: VirtualAddressOf<Arch>) -> Option<PhysicalAddressOf<Arch>> {
        unsafe { (*self.root_pt_ptr()).translate(va) }
    }

    fn root_pt_ptr(&self) -> *mut PageTable {
        phys_to_virt(self.root_pt.raw()) as *mut PageTable
    }
}

fn zero_frame(pa: PhysicalAddressOf<Arch>) {
    unsafe {
        ptr::write_bytes(phys_to_virt(pa.raw()) as *mut u8, 0, PAGE_SIZE);
    }
}
