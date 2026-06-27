use core::marker::PhantomData;

use crate::mm::VirtAddr;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct UserPtr<T> {
    addr: VirtAddr,
    _marker: PhantomData<T>,
}

impl<T> UserPtr<T> {
    #[must_use]
    pub const fn new(addr: usize) -> Self {
        Self {
            addr: VirtAddr::new(addr),
            _marker: PhantomData,
        }
    }

    #[must_use]
    pub const fn offset(&self, count: isize) -> Option<Self> {
        match self.addr.offset_by(count * (size_of::<T>() as isize)) {
            Some(v) => Some(Self::new(v.raw())),
            None => None,
        }
    }

    #[must_use]
    pub const fn is_null(&self) -> bool {
        self.addr.raw() == 0
    }

    #[must_use]
    pub const fn raw(&self) -> usize {
        self.addr.raw()
    }

    /// Copies from userspace to kernel. Note that this copy is shallow.
    /// Safety:
    ///  - `self` is valid in the current active page table.
    ///  - `dest` is an aligned kernel ptr to `T`.
    pub unsafe fn copy_from_user(&self, dest: &mut T) {
        let dest = dest as *mut T;
        for i in 0..size_of::<T>() {
            unsafe {
                let b = *((self.raw() + i) as *const u8);
                *(dest as *mut u8).add(i) = b;
            }
        }
    }

    /// Copies from kernel to userspace. Note that this copy is shallow.
    /// Safety:
    ///  - `self` is valid in the current active page table.
    ///  - `src` is an aligned kernel ptr to `T`.
    pub unsafe fn copy_into_user(&self, src: &T) {
        let src = src as *const T;
        for i in 0..size_of::<T>() {
            unsafe {
                let b = *(src as *const u8).add(i);
                *((self.raw() + i) as *mut u8) = b;
            }
        }
    }
}
