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
        let (pte, page_size) = self.translate_to_pte(va)?;
        Some(Self::translated_physical_address(pte, page_size, va))
    }

    fn translate_to_pte(&self, va: VirtualAddress) -> Option<(&PageTableEntry, PageSize)> {
        let (pte, page_size) = Self::translate_to_pte_ptr(self, va)?;
        Some((unsafe { pte.as_ref().unwrap() }, page_size))
    }

    pub fn translate_mut(&mut self, va: VirtualAddress) -> Option<(&mut PageTableEntry, PageSize)> {
        let (pte, page_size) = Self::translate_to_pte_ptr(self, va)?;
        Some((unsafe { pte.cast_mut().as_mut().unwrap() }, page_size))
    }

    fn translate_to_pte_ptr(
        root: *const PageTable,
        va: VirtualAddress,
    ) -> Option<(*const PageTableEntry, PageSize)> {
        let indices = [va.vpn_2(), va.vpn_1(), va.vpn_0()];
        let page_sizes = [PageSize::Size1G, PageSize::Size2M, PageSize::Size4K];
        let mut page_table = root;

        for (index, page_size) in indices.into_iter().zip(page_sizes) {
            let pte = unsafe { (*page_table).0.get(index)? };
            if !pte.is_valid() {
                return None;
            }
            if pte.is_leaf() {
                return Some((pte, page_size));
            }

            page_table = (pte.physical_address().raw() + KERNEL_DIRECT_MAPPING_BASE.raw())
                as *const PageTable;
        }

        None
    }

    fn translated_physical_address(
        pte: &PageTableEntry,
        page_size: PageSize,
        va: VirtualAddress,
    ) -> PhysicalAddress {
        unsafe {
            PhysicalAddress::from_raw_unchecked(
                pte.physical_address().raw() + (va.raw() & (page_size.bytes() - 1)),
            )
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

    pub fn fork(root_pt: PhysicalAddress) -> PhysicalAddress {
        let root_ptr = mm::phys_to_virt(root_pt.raw()) as *mut PageTable;

        let copy_pa = mm::alloc_frame().expect("NoMem");
        let copy_ptr = mm::phys_to_virt(copy_pa.raw()) as *mut PageTable;
        unsafe {
            core::ptr::copy_nonoverlapping(root_ptr.cast_const(), copy_ptr, 1);
        }

        let root = unsafe { root_ptr.as_mut().unwrap() };
        let copy = unsafe { copy_ptr.as_mut().unwrap() };
        for (parent_pte, child_pte) in root.0.iter_mut().zip(copy.0.iter_mut()) {
            if !parent_pte.is_valid() {
                continue;
            }

            if parent_pte.is_leaf() {
                if parent_pte.is_user() {
                    mm::page::add(parent_pte.physical_address().into());

                    if parent_pte.is_writable() {
                        *parent_pte = parent_pte.unset_flags(PteFlags::W);
                        *child_pte = child_pte.unset_flags(PteFlags::W);
                    }
                }
                continue;
            }

            // child page table
            let child = parent_pte.physical_address();
            let pa = Self::fork(child.into());
            *child_pte = child_pte.set_physical_address(pa.into());
        }

        copy_pa.into()
    }

    pub fn copy_on_write(root_pt: PhysicalAddress, addr: VirtualAddress) {
        let root_pt = unsafe {
            // TODO(aeryz): we don't wanna panic here
            (mm::phys_to_virt(root_pt.raw()) as *mut PageTable)
                .as_mut()
                .expect("root_pt is valid")
        };

        let (pte, page_size) = root_pt
            .translate_mut(addr)
            .expect("we shouldn't go into path when the page does not exist");

        if page_size != PageSize::Size4K {
            panic!(
                "Our kernel cannot handle CoW on pages larger than 4k as of now because the author is lazy."
            );
        }

        // NOTE: This only works because we explicitly don't support page sizes other
        // than 4k.
        let old_pa = pte.physical_address();
        let new_pa = mm::alloc_frame().expect("NoMem");

        unsafe {
            core::ptr::copy_nonoverlapping(
                mm::phys_to_virt(old_pa.raw()) as *const u8,
                mm::phys_to_virt(new_pa.raw()) as *mut u8,
                page_size.bytes(),
            );
        }

        mm::page::add(new_pa);
        *pte = pte
            .set_physical_address(new_pa.into())
            .set_flags(PteFlags::W);

        if mm::page::remove(old_pa.into()).expect("mapped user pages have metadata") == 0 {
            mm::free_frame(old_pa.into());
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
