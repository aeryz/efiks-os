use core::marker::PhantomData;

use crate::mm::VirtAddr;

#[derive(Debug, PartialEq, Eq)]
pub struct UserPtr<T> {
    addr: VirtAddr,
    _marker: PhantomData<fn() -> T>,
}

impl<T> Copy for UserPtr<T> {}

impl<T> Clone for UserPtr<T> {
    fn clone(&self) -> Self {
        Self {
            addr: self.addr,
            _marker: self._marker,
        }
    }
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

    pub unsafe fn copy_from_user_many_until(
        &self,
        dest: &mut [T],
        should_stop: fn(*const T) -> bool,
    ) -> Option<usize> {
        let mut cur_ptr = self.raw();
        for dest_idx in 0..dest.len() {
            if should_stop(cur_ptr as *const T) {
                return Some(dest_idx);
            }
            let dest_item = &mut dest[dest_idx];
            let dest_ptr = dest_item as *mut T;
            for i in 0..size_of::<T>() {
                unsafe {
                    let b = *((cur_ptr + i) as *const u8);
                    *(dest_ptr as *mut u8).add(i) = b;
                }
            }
            cur_ptr += size_of::<T>();
        }

        None
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

impl<T> UserPtr<UserPtr<T>> {
    #[must_use]
    pub fn iter(&self) -> UserPtrIterator<T> {
        UserPtrIterator { cur: *self }
    }
}

pub struct UserPtrIterator<T> {
    cur: UserPtr<UserPtr<T>>,
}

impl<T> Iterator for UserPtrIterator<T> {
    type Item = UserPtr<T>;

    fn next(&mut self) -> Option<Self::Item> {
        let next = self.cur.offset(1)?;
        let mut ret = UserPtr::new(0);
        unsafe {
            self.cur.copy_from_user(&mut ret);
        }
        self.cur = next;
        Some(ret)
    }
}
