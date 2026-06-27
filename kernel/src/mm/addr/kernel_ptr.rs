use core::marker::PhantomData;

use crate::{error::Error, mm::VirtAddr};

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct KernelPtr<T> {
    addr: VirtAddr,
    _marker: PhantomData<T>,
}

impl<T> KernelPtr<T> {
    pub const NULL: Self = Self {
        addr: VirtAddr::ZERO,
        _marker: PhantomData,
    };

    /// Creates an aligned pointer from `addr`.
    ///
    /// Returns `Err` if the pointer is not aligned or `null`.
    #[must_use]
    pub const fn new(addr: VirtAddr) -> Result<Self, Error> {
        if addr.raw() == 0 || addr.raw() % core::mem::align_of::<T>() == 0 {
            Ok(Self {
                addr,
                _marker: PhantomData,
            })
        } else {
            Err(Error::Unaligned)
        }
    }

    #[must_use]
    pub const fn as_ptr(&self) -> *const T {
        self.addr() as *const T
    }

    #[must_use]
    pub const fn as_ptr_mut(&self) -> *mut T {
        self.addr() as *mut T
    }

    #[must_use]
    pub const fn addr(&self) -> usize {
        self.addr.raw()
    }
}
