use crate::{arch::mmu::PhysicalAddress, mm::address::VirtAddr};

pub const KERNEL_IMAGE_START_VA: VirtAddr = VirtAddr::new(0xffff_ffff_8000_0000);

pub const KERNEL_IMAGE_START_PA: PhysicalAddress =
    unsafe { PhysicalAddress::from_raw_unchecked(0x8000_0000) };

pub const fn virt_to_phys(virt: usize) -> usize {
    virt - KERNEL_DIRECT_MAPPING_BASE.raw()
}

pub const fn phys_to_virt(phys: usize) -> usize {
    phys + KERNEL_DIRECT_MAPPING_BASE.raw()
}

pub const fn kernel_text_virt_to_phys_raw(virt: usize) -> PhysicalAddress {
    unsafe { PhysicalAddress::from_raw_unchecked(virt & 0xffff_ffff) }
}

pub const KERNEL_DIRECT_MAPPING_BASE: VirtAddr = VirtAddr::new(0xffff_ffd6_0000_0000);
