use crate::arch::{
    PageSize,
    mmu::{PageTableEntry, PteFlags, VirtualAddress},
};
use crate::mm::{self, KERNEL_DIRECT_MAPPING_BASE};

use super::PhysicalAddress;

#[repr(C, align(4096))]
pub struct PageTable([PageTableEntry; 512]);

impl PageTable {
    pub const fn empty() -> Self {
        PageTable([PageTableEntry::empty(); 512])
    }

    pub const fn set_entry(&mut self, idx: usize, entry: PageTableEntry) {
        self.0[idx] = entry;
    }

    /// Map the `va` to `pa`.
    ///
    /// This only meant to operate when the virtual memory is not enabled.
    pub fn map_vm_early(&mut self, va: VirtualAddress, pa: PhysicalAddress, flags: PteFlags) {
        self.map_memory_with_base(va, Some(pa), flags, 0, PageSize::Size4K);
    }

    /// Map a 2 MiB page from `va` to `pa`.
    ///
    /// This only meant to operate when the virtual memory is not enabled.
    pub fn map_vm_early_2m(&mut self, va: VirtualAddress, pa: PhysicalAddress, flags: PteFlags) {
        self.map_memory_with_base(va, Some(pa), flags, 0, PageSize::Size2M);
    }

    /// Map a 1 GiB page from `va` to `pa`.
    ///
    /// This only meant to operate when the virtual memory is not enabled.
    pub fn map_vm_early_1g(&mut self, va: VirtualAddress, pa: PhysicalAddress, flags: PteFlags) {
        self.map_memory_with_base(va, Some(pa), flags, 0, PageSize::Size1G);
    }

    /// Map the `va` to `pa`.
    ///
    /// This should be used after the virtual memory is enabled and the kvm
    /// mappings are done.
    pub fn map_vm(&mut self, va: VirtualAddress, pa: PhysicalAddress, flags: PteFlags) {
        self.map_vm_with_page_size(va, pa, flags, PageSize::Size4K);
    }

    /// Map the `va` to `pa` with `page_size`.
    ///
    /// This should be used after the virtual memory is enabled and the kvm
    /// mappings are done.
    pub fn map_vm_with_page_size(
        &mut self,
        va: VirtualAddress,
        pa: PhysicalAddress,
        flags: PteFlags,
        page_size: PageSize,
    ) {
        self.map_memory_with_base(
            va,
            Some(pa),
            flags,
            KERNEL_DIRECT_MAPPING_BASE.raw() as usize,
            page_size,
        );
    }

    /// Map a 2 MiB page from `va` to `pa`.
    ///
    /// This should be used after the virtual memory is enabled and the kvm
    /// mappings are done.
    pub fn map_vm_2m(&mut self, va: VirtualAddress, pa: PhysicalAddress, flags: PteFlags) {
        self.map_memory_with_base(
            va,
            Some(pa),
            flags,
            KERNEL_DIRECT_MAPPING_BASE.raw() as usize,
            PageSize::Size2M,
        );
    }

    /// Map a 1 GiB page from `va` to `pa`.
    ///
    /// This should be used after the virtual memory is enabled and the kvm
    /// mappings are done.
    pub fn map_vm_1g(&mut self, va: VirtualAddress, pa: PhysicalAddress, flags: PteFlags) {
        self.map_memory_with_base(
            va,
            Some(pa),
            flags,
            KERNEL_DIRECT_MAPPING_BASE.raw() as usize,
            PageSize::Size1G,
        );
    }

    /// Map the `va`
    pub fn remap_vm(&mut self, va: VirtualAddress, flags: PteFlags) {
        self.remap_memory_with_base(va, flags, KERNEL_DIRECT_MAPPING_BASE.raw() as usize);
    }

    pub fn translate(&self, va: VirtualAddress) -> Option<PhysicalAddress> {
        let l2_entry = self.0.get(va.vpn_2())?;
        if !l2_entry.is_valid() {
            return None;
        }
        if l2_entry.is_leaf() {
            return Some(unsafe {
                PhysicalAddress::from_raw_unchecked(
                    l2_entry.physical_address().raw() + (va.raw() & (PageSize::Size1G.bytes() - 1)),
                )
            });
        }

        let l1_pt = (l2_entry.physical_address().raw() + KERNEL_DIRECT_MAPPING_BASE.raw())
            as *const PageTable;

        let l1_entry = unsafe { (*l1_pt).0.get_unchecked(va.vpn_1()) };
        if !l1_entry.is_valid() {
            return None;
        }
        if l1_entry.is_leaf() {
            return Some(unsafe {
                PhysicalAddress::from_raw_unchecked(
                    l1_entry.physical_address().raw() + (va.raw() & (PageSize::Size2M.bytes() - 1)),
                )
            });
        }

        let l0_pt = (l1_entry.physical_address().raw() + KERNEL_DIRECT_MAPPING_BASE.raw())
            as *const PageTable;

        let l0_entry = unsafe { (*l0_pt).0.get_unchecked(va.vpn_0()) };
        if l0_entry.is_valid() && l0_entry.is_leaf() {
            Some(unsafe {
                PhysicalAddress::from_raw_unchecked(
                    l0_entry.physical_address().raw() + (va.raw() & (PageSize::Size4K.bytes() - 1)),
                )
            })
        } else {
            None
        }
    }

    pub fn traverse_free(root_pt: PhysicalAddress) {
        let root = mm::phys_to_virt(root_pt.raw()) as *const PageTable;
        for pte in unsafe { root.as_ref().unwrap().0 } {
            if !pte.is_valid() || pte.is_leaf() {
                continue;
            }

            // child page table
            let child = pte.physical_address();
            Self::traverse_free(child);
        }

        mm::free_frame(root_pt.into());
    }

    fn traverse_mut(&mut self, base: usize, cb: fn(leaf_pt: *mut PageTable)) {
        self.recursive_traverse_mut(base, 3, cb);
    }

    fn recursive_traverse_mut(
        &mut self,
        base: usize,
        depth: usize,
        cb: fn(leaf_pt: *mut PageTable),
    ) {
        for pte in &mut self.0 {
            if !pte.is_valid() || pte.is_leaf() {
                continue;
            }

            let pt = (pte.physical_address().raw() + base) as *mut PageTable;
            unsafe {
                pt.as_mut()
                    .unwrap()
                    .recursive_traverse_mut(base, depth - 1, cb);
            }
        }
    }

    fn map_memory_with_base(
        &mut self,
        va: VirtualAddress,
        pa: Option<PhysicalAddress>,
        flags: PteFlags,
        base: usize,
        page_size: PageSize,
    ) {
        Self::check_mapping_alignment(va, pa, page_size);

        let l2_entry = &mut self.0[va.vpn_2()];
        if let PageSize::Size1G = page_size {
            Self::map_leaf(l2_entry, pa, flags);
            return;
        }

        let l1_page_table = Self::get_or_create_next_table(l2_entry, base);

        let l1_entry = unsafe { (*l1_page_table).0.get_unchecked_mut(va.vpn_1()) };
        if let PageSize::Size2M = page_size {
            Self::map_leaf(l1_entry, pa, flags);
            return;
        }

        let l0_page_table = Self::get_or_create_next_table(l1_entry, base);

        let l0_entry = unsafe { (*l0_page_table).0.get_unchecked_mut(va.vpn_0()) };
        Self::map_leaf(l0_entry, pa, flags);
    }

    fn remap_memory_with_base(&mut self, va: VirtualAddress, flags: PteFlags, base: usize) {
        let l2_entry = &mut self.0[va.vpn_2()];
        if !l2_entry.is_valid() {
            panic!("trying to remap an unmapped vm");
        }
        if l2_entry.is_leaf() {
            Self::map_leaf(l2_entry, None, flags);
            return;
        }

        let l1_page_table = (l2_entry.physical_address().raw() + base) as *mut PageTable;
        let l1_entry = unsafe { (*l1_page_table).0.get_unchecked_mut(va.vpn_1()) };
        if !l1_entry.is_valid() {
            panic!("trying to remap an unmapped vm");
        }
        if l1_entry.is_leaf() {
            Self::map_leaf(l1_entry, None, flags);
            return;
        }

        let l0_page_table = (l1_entry.physical_address().raw() + base) as *mut PageTable;
        let l0_entry = unsafe { (*l0_page_table).0.get_unchecked_mut(va.vpn_0()) };
        if !l0_entry.is_valid() || !l0_entry.is_leaf() {
            panic!("trying to remap an unmapped vm");
        }
        Self::map_leaf(l0_entry, None, flags);
    }

    fn map_leaf(pte: &mut PageTableEntry, pa: Option<PhysicalAddress>, flags: PteFlags) {
        if pte.is_valid() && !pte.is_leaf() {
            panic!("trying to replace a page table with a leaf mapping");
        }

        if !pte.is_valid() {
            if let Some(pa) = pa {
                *pte = pte.set_physical_address(pa);
            } else {
                // TODO(aeryz): make this API return an error
                panic!("trying to remap a vm with pa = None");
            }
        }
        *pte = pte.set_flags(flags | PteFlags::V | PteFlags::A | PteFlags::D);
    }

    fn check_mapping_alignment(
        va: VirtualAddress,
        pa: Option<PhysicalAddress>,
        page_size: PageSize,
    ) {
        match page_size {
            PageSize::Size4K => {
                debug_assert!(va.raw() & (page_size.bytes() - 1) == 0);
                if let Some(pa) = pa {
                    debug_assert!(pa.is_4k_page_aligned());
                }
            }
            PageSize::Size2M => {
                debug_assert!(va.raw() & (page_size.bytes() - 1) == 0);
                if let Some(pa) = pa {
                    debug_assert!(pa.is_2m_page_aligned());
                }
            }
            PageSize::Size1G => {
                debug_assert!(va.raw() & (page_size.bytes() - 1) == 0);
                if let Some(pa) = pa {
                    debug_assert!(pa.is_1g_page_aligned());
                }
            }
        }
    }

    fn get_or_create_next_table(pte: &mut PageTableEntry, base: usize) -> *mut PageTable {
        if pte.is_valid() {
            if pte.is_leaf() {
                panic!("trying to create a child page table under a leaf mapping");
            }
            return (pte.physical_address().raw() + base) as *mut PageTable;
        }

        let pa = mm::alloc_frame().unwrap();
        let va = VirtualAddress::from_raw(pa.raw() + base).unwrap();
        let page_table_ptr = va.as_ptr_mut();
        unsafe {
            *page_table_ptr = PageTable::empty();
        }
        *pte = PageTableEntry::new_valid().set_physical_address(pa.into());
        page_table_ptr
    }
}
