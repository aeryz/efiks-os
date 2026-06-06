use core::arch::asm;

use crate::{
    arch::mmu::{PageTable, PageTableEntry, PhysicalAddress, PteFlags},
    kmain, mm,
};

static EARLY_PT: PageTable = {
    let mut page_table = PageTable::empty();
    let pte = PageTableEntry::empty()
        .set_flags(PteFlags::V.union(PteFlags::RWX).union(PteFlags::A).union(PteFlags::D));

    page_table.set_entry(
        (mm::KERNEL_IMAGE_START_PA.raw() >> 30) & 0x1ff,
        pte.set_physical_address(mm::KERNEL_IMAGE_START_PA),
    );

    page_table.set_entry(
        510,
        pte.set_physical_address(mm::KERNEL_IMAGE_START_PA),
    );

    page_table.set_entry(
        511,
        pte.set_physical_address(unsafe {
            PhysicalAddress::from_raw_unchecked(mm::KERNEL_IMAGE_START_PA.raw() + mm::GB)
        }),
    );

    page_table
};

#[unsafe(no_mangle)]
pub extern "C" fn bootentry(hart_id: usize, dtb_pa: usize) -> ! {
    unsafe {
        asm!(
            "la t0, {early_pt}",
            "srli t0, t0, 12",
            "li t1, {satp_mode}",
            "or t0, t0, t1",
            "csrw satp, t0",
            "sfence.vma zero, zero",
            "li t0, {kernel_offset}",
            "add t0, t0, {}",
            "mv a0, {}",
            "mv a1, {}",
            "jr t0",
            in(reg) kmain as *const () as u64,
            in(reg) hart_id,
            in(reg) dtb_pa,
            early_pt = sym EARLY_PT,
            satp_mode = const (8usize << 60),
            kernel_offset = const (mm::KERNEL_IMAGE_START_VA.raw() - mm::KERNEL_IMAGE_START_PA.raw()),
            options(noreturn, nostack, preserves_flags))
    }
}
