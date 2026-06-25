use ksync::SpinLock;

use crate::{arch::mmu::PhysicalAddress, error::Error};

static FRAME_ALLOCATOR: SpinLock<FrameAllocator<64>> = SpinLock::new(FrameAllocator::new(unsafe {
    PhysicalAddress::from_raw_unchecked(0)
}));

/// Initialize the allocator with the given `start_addr`.
pub(super) fn init(start_addr: PhysicalAddress) {
    FRAME_ALLOCATOR.lock().start_addr = start_addr;
}

// TODO: be able to allocate a custom amount (will probably have this while
// implementing sbrk)
/// Allocate a single 4k page.
pub fn alloc_frame() -> Result<PhysicalAddress, Error> {
    FRAME_ALLOCATOR.lock().alloc()
}

#[allow(unused)]
/// Free a single 4k page.
pub fn free_frame(ptr: PhysicalAddress) {
    FRAME_ALLOCATOR.lock().free(ptr)
}

/// Dumb allocator
///
/// Starting from the `start_addr`, support 64 * N pages.
///
/// This allocator only works with maximum of 4K alloc requests.
/// It doesn't have a smart mechanism to walk through the pages.
/// It just tries to find the next empty table by going through
/// the `pages` bitfields.
#[repr(C)]
struct FrameAllocator<const N: usize> {
    start_addr: PhysicalAddress,
    pages: [u64; N],
}

impl<const N: usize> FrameAllocator<N> {
    #[must_use]
    const fn new(start_addr: PhysicalAddress) -> Self {
        Self {
            start_addr,
            pages: [0; N],
        }
    }

    /// Allocates a single page and returns its start address
    ///
    /// Returns `Err` if there is no memory left.
    #[must_use]
    fn alloc(&mut self) -> Result<PhysicalAddress, Error> {
        for page_i in 0..self.pages.len() {
            for i in 0..(u64::BITS as usize) {
                if (self.pages[page_i] >> i) & 1 == 0 {
                    self.pages[page_i] |= 1 << i;
                    let alloc_addr = (page_i * u64::BITS as usize + i) * 4096;
                    // Safety:
                    // - TODO: we must guarantee that the allocator is properly configured s.t.
                    // we can't produce an out of bounds physical address
                    return Ok(unsafe {
                        PhysicalAddress::from_raw_unchecked(self.start_addr.raw() + alloc_addr)
                    });
                }
            }
        }

        Err(Error::Oom)
    }

    fn free(&mut self, ptr: PhysicalAddress) {
        let x = (ptr.raw() - self.start_addr.raw()) / 4096;
        let page_i = x / 64;
        let i = x % 64;

        self.pages[page_i as usize] &= !(1 << i);
    }
}
