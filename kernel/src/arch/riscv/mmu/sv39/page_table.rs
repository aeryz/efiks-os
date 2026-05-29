use crate::arch::mmu::{PageTableEntry, PteFlags, VirtualAddress};
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
        self.map_memory_with_base(va, Some(pa), flags, 0);
    }

    /// Map the `va` to `pa`.
    ///
    /// This should be used after the virtual memory is enabled and the kvm
    /// mappings are done.
    pub fn map_vm(&mut self, va: VirtualAddress, pa: PhysicalAddress, flags: PteFlags) {
        self.map_memory_with_base(
            va,
            Some(pa),
            flags,
            KERNEL_DIRECT_MAPPING_BASE.raw() as usize,
        );
    }

    /// Map the `va`
    pub fn remap_vm(&mut self, va: VirtualAddress, flags: PteFlags) {
        self.map_memory_with_base(va, None, flags, KERNEL_DIRECT_MAPPING_BASE.raw() as usize);
    }

    pub fn translate(&self, va: VirtualAddress) -> Option<PhysicalAddress> {
        let l1_pt = (self.0[va.vpn_2()].physical_address().raw() + KERNEL_DIRECT_MAPPING_BASE.raw())
            as *const PageTable;

        let l0_pt = unsafe {
            ((*l1_pt).0[va.vpn_1()].physical_address().raw() + KERNEL_DIRECT_MAPPING_BASE.raw())
                as *const PageTable
        };

        let l0_entry = unsafe { (*l0_pt).0.get_unchecked(va.vpn_0()) };

        if l0_entry.is_valid() {
            Some(l0_entry.physical_address())
        } else {
            None
        }
    }

    fn map_memory_with_base(
        &mut self,
        va: VirtualAddress,
        pa: Option<PhysicalAddress>,
        flags: PteFlags,
        base: usize,
    ) {
        let l2_entry = &mut self.0[va.vpn_2()];

        let l1_page_table = Self::get_or_create_next_table(l2_entry, base);

        let l1_entry = unsafe { (*l1_page_table).0.get_unchecked_mut(va.vpn_1()) };
        let l0_page_table = Self::get_or_create_next_table(l1_entry, base);

        let l0_entry = unsafe { (*l0_page_table).0.get_unchecked_mut(va.vpn_0()) };
        if !l0_entry.is_valid() {
            if let Some(pa) = pa {
                *l0_entry = l0_entry.set_physical_address(pa);
            } else {
                // TODO(aeryz): make this API return an error
                panic!("trying to remap a vm with pa = None");
            }
        }
        *l0_entry = l0_entry.set_flags(flags | PteFlags::V | PteFlags::A | PteFlags::D);
    }

    fn get_or_create_next_table(pte: &mut PageTableEntry, base: usize) -> *mut PageTable {
        if pte.is_valid() {
            return (pte.physical_address().raw() + base) as *mut PageTable;
        }

        let pa = mm::alloc_frame().unwrap();
        let va = VirtualAddress::from_raw(pa.raw() + base).unwrap();
        let page_table_ptr = va.as_ptr_mut();
        unsafe {
            *page_table_ptr = PageTable::empty();
        }
        *pte = PageTableEntry::new_valid().set_physical_address(pa);
        page_table_ptr
    }
}
