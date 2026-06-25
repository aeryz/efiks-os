use crate::{error::Error, helper};

#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct VirtAddr(usize);

impl VirtAddr {
    pub const ZERO: Self = Self(0);

    pub fn align_up(&self, page_size: usize) -> Self {
        Self(helper::align_up(self.0, page_size))
    }

    #[must_use]
    pub const fn new(addr: usize) -> Self {
        Self(addr)
    }

    pub fn offset_by(&self, amount: usize) -> Option<Self> {
        self.0.checked_add(amount).map(Self::new)
    }
}

impl From<usize> for VirtAddr {
    fn from(value: usize) -> Self {
        Self(value)
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct KernelVirtAddr(VirtAddr);

impl KernelVirtAddr {
    pub fn new<A: Into<VirtAddr>>(addr: A) -> Result<Self, Error> {
        // TODO(aeryz): only accept if its a direct map?
        // How about kernel text? Maybe this should better be able to represent all high
        // base memory
        Ok(Self(addr.into()))
    }

    /// Returns error if the address is not aligned for `T`.
    pub fn as_ptr<T>(&self) -> Result<*const T, Error> {
        if self.check_aligned::<T>() {
            Ok(self.raw() as *const T)
        } else {
            Err(Error::Unaligned)
        }
    }

    /// Returns error if the address is not aligned for `T`.
    pub fn as_ptr_mut<T>(&self) -> Result<*mut T, Error> {
        if self.check_aligned::<T>() {
            Ok(self.raw() as *mut T)
        } else {
            Err(Error::Unaligned)
        }
    }

    #[must_use]
    #[inline(always)]
    pub fn check_aligned<T>(&self) -> bool {
        self.raw() % core::mem::align_of::<T>() == 0
    }

    #[must_use]
    #[inline(always)]
    fn raw(&self) -> usize {
        self.0.0
    }
}

pub struct PhysAddr(usize);
