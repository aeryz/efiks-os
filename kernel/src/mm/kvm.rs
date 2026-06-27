// TODO(aeryz): This module contains a lot of arch specific code since we full
// mapping of the memory. We need to extend `arch::MemoryModel` to handle these.

use crate::{
    Arch,
    arch::{Architecture, MemoryModel, MemoryModelOf, mmu::PteFlags},
    mm::{self, PhysAddr, VirtAddr, frame_allocator, kernel_allocator},
};
use ksync::SpinLock;

pub const KB: usize = 1 << 10;
pub const GB: usize = 1 << 30;

static KERNEL_ROOT_PAGE_TABLE: SpinLock<VirtAddr> = SpinLock::new(VirtAddr::ZERO);

unsafe extern "C" {
    static __text_start: u8;
    static __text_end: u8;
    static __kernel_end: u8;
}

/// Performs early memory initialization and enables paging.
///
/// This function *MUST* run in the early boot phase where the paging is not
/// enabled yet. All the pointers used here are assumed to be physical
/// addresses.
///
/// # Responsibilities
/// - Initializes the physical memory allocator starting from `__kernel_end`.
/// - Creates the kernel root page table with the following mappings:
///     - Whole memory is mapped with 1GB `RW` pages starting from
///       `mm::KERNEL_DIRECT_MAPPING_BASE`.
///     - The last 2GB of the memory is reserved for the kernel text and it's
///       mapped with 2 1GB `RX` pages.
///     - Kernel text is identity mapped so that we don't immediately trap after
///       changing `satp`.
/// - Enables the paging.
/// - Changes the stack pointer to the higher base (0x80...-> 0xffff80...).
///
/// # Safety
/// - Assumes the kernel's executable text is put directly at `__text_start` and
///   it ends in `__text_end`.
/// - Assumes the `__kernel_end` is put at the end of the kernel image and is
///   *4k-aligned*.
/// - Assumes this is a single-hart boot.
pub fn early_init() {
    // TODO(aeryz): We want to have a separate spot for the allocatable memory.
    let memory_start =
        unsafe { mm::kernel_text_virt_to_phys_raw(&__kernel_end as *const u8 as usize) };
    frame_allocator::init(memory_start);

    let mut root_pt = KERNEL_ROOT_PAGE_TABLE.lock();
    let root_pt_pa = mm::alloc_frame().unwrap();
    *root_pt = VirtAddr::new(root_pt_pa.raw());
    MemoryModelOf::<Arch>::initialize_empty_pt((*root_pt).into());

    let text_end = unsafe { mm::kernel_text_virt_to_phys_raw(&__text_end as *const u8 as usize) };
    let mut text_start =
        unsafe { mm::kernel_text_virt_to_phys_raw(&__text_start as *const u8 as usize) };
    let n_text_pages = (text_end.raw() - text_start.raw()) / KB + 1;

    kvm_full_map(*root_pt);
    for _ in 0..n_text_pages {
        MemoryModelOf::<Arch>::map_vm_early(
            (*root_pt).into(),
            VirtAddr::new(text_start.raw()).into(),
            text_start.into(),
            PteFlags::RWX,
        );
        text_start = text_start.offset_by(0x1000).unwrap();
    }

    Arch::set_root_page_table_pa(root_pt_pa.into());

    // 192K kernel heap
    {
        // TODO(aeryz): This depends on the assumption that the frame allocator will
        // provide contiguous memory
        let start_addr = frame_allocator::alloc_frame().unwrap();
        for _ in 0..46 {
            let _ = frame_allocator::alloc_frame().unwrap();
        }
        let end_addr = frame_allocator::alloc_frame().unwrap();

        kernel_allocator::init(
            VirtAddr::new(mm::phys_to_virt(start_addr.raw())),
            VirtAddr::new(mm::phys_to_virt(end_addr.raw()) + 4096),
        );
    }

    Arch::bump_sp(mm::KERNEL_DIRECT_MAPPING_BASE.raw());

    *root_pt = VirtAddr::new(mm::phys_to_virt(root_pt_pa.raw()));
}

/// Maps the whole memory starting from `mm::KERNEL_DIRECT_MAPPING_BASE` and
/// maps the kernel text as executable so that we don't need to switch page
/// tables during traps.
pub fn kvm_full_map(root_pt: VirtAddr) {
    let direct_mapping_size =
        mm::KERNEL_IMAGE_START_VA.difference(mm::KERNEL_DIRECT_MAPPING_BASE) as usize;
    debug_assert!(direct_mapping_size % GB == 0);

    for i in 0..(direct_mapping_size / GB) {
        let va = mm::KERNEL_DIRECT_MAPPING_BASE
            .offset_by((i * GB) as isize)
            .expect("this is already a bounded op");
        let pa = PhysAddr::new(i * GB);

        MemoryModelOf::<Arch>::map_vm_1g(root_pt.into(), va.into(), pa.into(), PteFlags::RW);
    }

    // kernel image
    // TODO(aeryz): for convenience, will just have 2 1GB RWX tables
    for i in 0..2 {
        let va = mm::KERNEL_IMAGE_START_VA
            .offset_by((i * GB) as isize)
            .expect("this is already a bounded op");
        let pa = mm::KERNEL_IMAGE_START_PA
            .offset_by((i * GB) as isize)
            .expect("already a bounded op");

        MemoryModelOf::<Arch>::map_vm_1g(root_pt.into(), va.into(), pa.into(), PteFlags::RWX);
    }
}
