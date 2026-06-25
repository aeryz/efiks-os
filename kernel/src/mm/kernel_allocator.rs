use core::{alloc::GlobalAlloc, mem::MaybeUninit};

use kmalloc::{KernelAllocator as _, LinkedListAllocator};
use ksync::SpinLock;

use crate::mm::KernelVirtAddr;

type KernelAllocator = LinkedListAllocator;

#[global_allocator]
static ALLOCATOR: Allocator = Allocator(SpinLock::new(MaybeUninit::uninit()));

struct Allocator(SpinLock<MaybeUninit<LinkedListAllocator>>);

unsafe impl Send for Allocator {}
unsafe impl Sync for Allocator {}

pub fn init(start_addr: KernelVirtAddr, end_addr: KernelVirtAddr) {
    let allocator = unsafe { KernelAllocator::new(start_addr.raw(), end_addr.raw()).unwrap() };
    *ALLOCATOR.0.lock() = MaybeUninit::new(allocator);
}

unsafe impl GlobalAlloc for Allocator {
    unsafe fn alloc(&self, layout: core::alloc::Layout) -> *mut u8 {
        unsafe { self.0.lock().assume_init_ref().alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: core::alloc::Layout) {
        unsafe { self.0.lock().assume_init_ref().dealloc(ptr, layout) }
    }
}
