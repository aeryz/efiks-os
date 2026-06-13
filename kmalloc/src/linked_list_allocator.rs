//! # Linked list allocator
//!
//! ## Layout
//! ```
//! [ header ][ maybe padding ][ back offset ][ data ]
//! ```
//! - **header**: Metadata of this node.
//! - **maybe padding**: Padding to satisfy the alignment of `data`.
//! - **back offset**: Having a pointer to `data`, the allocator needs to be
//!   able to access to `Header` to be able to free this node. Hence, the `back
//!   offset` is used to tell the allocator how far back the `header` is from
//!   the `data`.
//! - **data**: Data itself.

use core::{
    alloc::{GlobalAlloc, Layout},
    mem::align_of,
    ptr::NonNull,
};

use crate::KernelAllocator;

#[repr(C)]
pub struct LinkedListAllocator {
    start_addr: usize,
    end_addr: usize,
    head: *mut Header,
}

#[repr(C)]
struct Header {
    sz: usize,
    free: bool,
    next: Option<*mut Header>,
}

impl KernelAllocator for LinkedListAllocator {
    unsafe fn new(start_addr: usize, end_addr: usize) -> Result<Self, ()> {
        // The allocator should at least be able to fit a single header
        if start_addr.checked_add(size_of::<Header>()).ok_or(())? > end_addr {
            return Err(());
        }

        let head = unsafe { NonNull::new(start_addr as *mut Header).ok_or(())?.as_mut() };
        head.sz = end_addr - start_addr;
        head.free = true;
        head.next = None;
        Ok(Self {
            start_addr,
            end_addr,
            head,
        })
    }
}

impl LinkedListAllocator {
    // For debugging only
    #[allow(unused)]
    fn traverse(&self) {
        let mut total_size = 0;
        let mut cur_node_ptr = self.head.cast_const();

        while cur_node_ptr != core::ptr::null() {
            let header: &Header = unsafe { cur_node_ptr.as_ref().unwrap() };
            log::info!(
                "header: {cur_node_ptr:?}\n\t- sz: {}\n\t- free: {}\n\t- next: {:?}",
                header.sz,
                header.free,
                header.next.unwrap_or(core::ptr::null_mut())
            );
            total_size += header.sz;
            cur_node_ptr = header.next.unwrap_or(core::ptr::null_mut()).cast_const();
        }

        if total_size != self.end_addr - self.start_addr {
            panic!(
                "expected the traversed size ({total_size}) to match the total size: {}",
                self.end_addr - self.start_addr,
            );
        }
    }
}

unsafe impl GlobalAlloc for LinkedListAllocator {
    /// See the module docs to see the exact layout.
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let mut cur_node_ptr = self.head;

        loop {
            let cur_node = unsafe { cur_node_ptr.as_mut().unwrap() };

            let header_start = cur_node_ptr as usize;

            // data is aligned to it's own alignment, the extra `usize` comes from the back
            // offset
            let data_start = crate::align_up(
                header_start + size_of::<Header>() + size_of::<usize>(),
                layout.align(),
            );
            let back_offset_pos = data_start - size_of::<usize>();
            // tells us how far back the metadata is so that we can free.
            let back_offset = data_start - header_start;

            // Make `alloc_size` aligned for `Header` so that it the next header is being
            // put in an aligned position.
            let alloc_size = crate::align_up(back_offset + layout.size(), align_of::<Header>());

            if cur_node.free && cur_node.sz >= alloc_size {
                let prev_sz = cur_node.sz;
                let remaining = prev_sz - alloc_size;

                if remaining
                    >= crate::align_up(
                        size_of::<Header>() + size_of::<usize>() + 1,
                        align_of::<Header>(),
                    )
                {
                    cur_node.sz = alloc_size;
                    cur_node.free = false;

                    unsafe {
                        *(back_offset_pos as *mut usize) = back_offset;

                        let next_node_ptr =
                            (cur_node_ptr as *mut u8).add(alloc_size) as *mut Header;
                        *next_node_ptr = Header {
                            sz: remaining,
                            free: true,
                            next: cur_node.next,
                        };
                        cur_node.next = Some(next_node_ptr);
                    }
                } else {
                    cur_node.free = false;
                    // consume whole block
                    unsafe {
                        *(back_offset_pos as *mut usize) = back_offset;
                    }
                }

                return data_start as *mut u8;
            }

            cur_node_ptr = match cur_node.next {
                Some(next) => next,
                None => panic!("kernel ran out of memory"),
            }
        }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, _: Layout) {
        let back_offset = unsafe {
            let back_offset_ptr = ptr.byte_sub(size_of::<usize>()).cast::<usize>();
            if back_offset_ptr.is_null() {
                return;
            }

            *back_offset_ptr
        };

        let header = unsafe {
            match ptr.byte_sub(back_offset).cast::<Header>().as_mut() {
                Some(ptr) => ptr,
                None => return,
            }
        };

        header.free = true;
    }
}

#[cfg(test)]
mod tests {
    use core::{
        alloc::{AllocError, Allocator, GlobalAlloc, Layout},
        mem::align_of,
        ptr::NonNull,
    };
    use std::prelude::v1::*;
    use std::{
        collections::{BTreeMap, VecDeque},
        format,
    };

    use crate::{KernelAllocator, LinkedListAllocator};

    const HEAP_SIZE: usize = 1024 * 1024;

    #[repr(align(4096))]
    struct TestHeap([u8; HEAP_SIZE]);

    static mut HEAP: TestHeap = TestHeap([0; HEAP_SIZE]);

    unsafe impl Allocator for &LinkedListAllocator {
        fn allocate(&self, layout: Layout) -> Result<NonNull<[u8]>, AllocError> {
            let ptr = unsafe { GlobalAlloc::alloc(*self, layout) };
            let nn = NonNull::new(ptr).ok_or(AllocError)?;
            Ok(NonNull::slice_from_raw_parts(nn, layout.size()))
        }

        unsafe fn deallocate(&self, ptr: NonNull<u8>, layout: Layout) {
            unsafe { GlobalAlloc::dealloc(*self, ptr.as_ptr(), layout) }
        }
    }

    #[repr(align(64))]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    struct Align64(u8);

    #[repr(align(256))]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    struct Align256(u64);

    #[test]
    fn collections_can_use_linked_list_allocator_directly() {
        let start = unsafe { HEAP.0.as_mut_ptr() as usize };
        let end = start + HEAP_SIZE;

        let alloc = unsafe {
            LinkedListAllocator::new(start, end).expect("failed to initialize LinkedListAllocator")
        };

        let mut bytes = Vec::new_in(&alloc);
        for i in 0..4096 {
            bytes.push((i % 251) as u8);
        }
        assert_eq!(bytes.len(), 4096);
        assert_eq!(bytes[123], (123 % 251) as u8);
        assert_eq!((bytes.as_ptr() as usize) % align_of::<u8>(), 0);

        let mut a64 = Vec::new_in(&alloc);
        for i in 0..128 {
            a64.push(Align64(i as u8));
        }
        assert_eq!(a64[17], Align64(17));
        assert_eq!((a64.as_ptr() as usize) % align_of::<Align64>(), 0);

        let mut a256 = Vec::new_in(&alloc);
        for i in 0..32 {
            a256.push(Align256(i as u64));
        }
        assert_eq!(a256[9], Align256(9));
        assert_eq!((a256.as_ptr() as usize) % align_of::<Align256>(), 0);

        // VecDeque
        let mut dq = VecDeque::new_in(&alloc);
        for i in 0..1000 {
            dq.push_back(i);
        }
        for i in 0..200 {
            assert_eq!(dq.pop_front(), Some(i));
        }
        assert_eq!(dq.front(), Some(&200));
        assert_eq!(dq.back(), Some(&999));

        let mut map = BTreeMap::new_in(&alloc);
        for i in 0..500 {
            map.insert(i, format!("value-{i}"));
        }
        assert_eq!(map.get(&77).map(String::as_str), Some("value-77"));
        assert_eq!(map.get(&499).map(String::as_str), Some("value-499"));

        drop(bytes);
        drop(a64);
        drop(a256);
        drop(dq);
        drop(map);

        let mut after = Vec::new_in(&alloc);
        for i in 0..256u128 {
            after.push(i);
        }
        assert_eq!(after[255], 255);
        assert_eq!((after.as_ptr() as usize) % align_of::<u128>(), 0);
    }
}
