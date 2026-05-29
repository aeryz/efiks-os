mod frame_allocator;
mod kernel_allocator;
mod kvm;
mod mappings;

#[allow(unused)]
pub use frame_allocator::{alloc_frame, free_frame};
pub use kvm::*;
pub use mappings::*;

pub const PAGE_SIZE: usize = 4096;
