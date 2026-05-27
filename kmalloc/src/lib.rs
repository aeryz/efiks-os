#![no_std]
#![cfg_attr(test, feature(allocator_api, btreemap_alloc))]
#![cfg_attr(test, allow(static_mut_refs))]

#[cfg(test)]
extern crate std;

mod linked_list_allocator;

use core::alloc::GlobalAlloc;

pub use linked_list_allocator::*;

pub trait KernelAllocator: GlobalAlloc + Sized {
    /// Creates a new allocator
    ///
    /// * `start_addr`: The start of the address that is reserved for this
    ///   allocator.
    /// * `end_addr`: The end of the address that is reserved for this
    ///   allocator.
    ///
    /// ## Safety
    /// - `start_addr` and `end_addr` are valid addresses during the execution
    ///   of this allocator.
    /// It's a really common mistake to initialize this allocator with physical
    /// addresses before starting paging and then immediately get a trap
    /// once paging is enabled.
    unsafe fn new(start_addr: usize, end_addr: usize) -> Result<Self, ()>;
}

pub(crate) fn align_up(addr: usize, align: usize) -> usize {
    (addr + align - 1) & !(align - 1)
}
