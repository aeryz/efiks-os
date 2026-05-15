use crate::arch::mmu::{PhysicalAddress, VirtualAddress};

pub const KERNEL_IMAGE_START_VA: VirtualAddress =
    unsafe { VirtualAddress::from_raw_unchecked(0xffff_ffff_8000_0000) };

pub const KERNEL_IMAGE_START_PA: PhysicalAddress =
    unsafe { PhysicalAddress::from_raw_unchecked(0x8000_0000) };

pub const KERNEL_DIRECT_MAPPING_BASE: VirtualAddress =
    unsafe { VirtualAddress::from_raw_unchecked(0xffff_ffd6_0000_0000) };

pub const fn virt_to_phys(virt: usize) -> usize {
    virt - KERNEL_DIRECT_MAPPING_BASE.raw()
}

pub const fn phys_to_virt(phys: usize) -> usize {
    phys + KERNEL_DIRECT_MAPPING_BASE.raw()
}
