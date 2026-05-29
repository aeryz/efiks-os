use alloc::vec::Vec;

use crate::{
    Arch,
    arch::{
        PhysicalAddressOf, VirtualAddressOf,
        mmu::{PageTable, PhysicalAddress, PteFlags, VirtualAddress},
    },
    mm::{self, PAGE_SIZE},
};

pub const ADDRESS_SPACE_EMPTY: AddressSpace = AddressSpace {
    root_pt: PhysicalAddress::ZERO,
    regions: Vec::new(),
};

#[allow(unused)]
#[derive(Clone)]
pub struct VmRegion {
    /// Start address of this region
    pub start: VirtualAddressOf<Arch>,
    /// End address of this region
    pub end: VirtualAddressOf<Arch>,
}

#[derive(Clone)]
pub struct AddressSpace {
    pub root_pt: PhysicalAddressOf<Arch>,
    pub regions: Vec<VmRegion>,
}

// TODO(aeryz): No way of allocating and mapping large tables. Idk how this
// should be done actually. Should this be the call of the `AddressSpace` where
// for example, when loading a single (2MB + 10 4KB) page, should this
// dynamically figure that it should reserve 1 2MB and 10 4KB pages?
impl AddressSpace {
    /// Creates a new address space for the user tasks
    pub fn new_user() -> Self {
        let root_pt = mm::alloc_frame().unwrap();

        let self_ = Self {
            root_pt,
            regions: Vec::new(),
        };

        unsafe {
            *(self_.root_pt_ptr()) = PageTable::empty();
            mm::kvm_full_map(self_.root_pt_ptr().as_mut().unwrap());
        }

        self_
    }

    pub fn create_user_stack(&mut self) -> VirtualAddressOf<Arch> {
        // 32KB user stack
        let mut va = unsafe { VirtualAddress::from_raw_unchecked(0x0000_0000_3fff_0000) };
        for _ in 0..8 {
            let user_stack = mm::alloc_frame().unwrap();

            va = VirtualAddress::from_raw(va.raw() + PAGE_SIZE).unwrap();
            unsafe { (*self.root_pt_ptr()).map_vm(va, user_stack, PteFlags::RW | PteFlags::U) };
            let _ = self.regions.push(VmRegion {
                start: va,
                end: VirtualAddress::from_raw(va.raw() + PAGE_SIZE).unwrap(),
            });
        }

        VirtualAddress::from_raw(va.raw() + PAGE_SIZE - 0x60).unwrap()
    }

    pub fn create_kernel_stack(&mut self) -> PhysicalAddressOf<Arch> {
        let mut kernel_stack_top = PhysicalAddress::ZERO;
        // 32KB kernel stack
        for _ in 0..8 {
            // TODO(aeryz): With the following logic, we cannot guarantee a 16KB contiguous
            // virtual memory. This is not acceptable.
            let kernel_stack = mm::alloc_frame().unwrap();
            let kernel_stack_va =
                VirtualAddress::from_raw(mm::phys_to_virt(kernel_stack.raw())).unwrap();
            let _ = self.regions.push(VmRegion {
                start: kernel_stack_va,
                end: VirtualAddress::from_raw(kernel_stack_va.raw() + 4096).unwrap(),
            });

            // We don't do mapping here because we already did `kvm_full_map` which maps the
            // entire memory with 1GB pages

            kernel_stack_top = PhysicalAddress::from_raw(kernel_stack.raw() + 0xfa0).unwrap();
        }

        kernel_stack_top
    }

    /// Allocates a single 4k physical page, maps `va` to it and adds it to the
    /// regions.
    pub fn map_allocate_page(
        &mut self,
        va: VirtualAddressOf<Arch>,
        flags: PteFlags,
    ) -> PhysicalAddressOf<Arch> {
        let pa = mm::alloc_frame().unwrap();

        unsafe {
            (*self.root_pt_ptr()).map_vm(va, pa, flags);
        };

        self.regions.push(VmRegion {
            start: va,
            end: unsafe { VirtualAddress::from_raw_unchecked(va.raw() + PAGE_SIZE) },
        });

        pa
    }

    /// Remap a previously mapped page. This is used for overriding the flags
    /// basically.
    pub fn remap_page(&mut self, va: VirtualAddressOf<Arch>, flags: PteFlags) {
        unsafe {
            (*self.root_pt_ptr()).remap_vm(va, flags);
        };
    }

    pub fn translate(&self, va: VirtualAddressOf<Arch>) -> Option<PhysicalAddressOf<Arch>> {
        unsafe { (*self.root_pt_ptr()).translate(va) }
    }

    fn root_pt_ptr(&self) -> *mut PageTable {
        mm::phys_to_virt(self.root_pt.raw()) as *mut PageTable
    }
}
