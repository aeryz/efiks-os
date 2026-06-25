use crate::{error::Error, helper};

#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct VirtAddr(usize);

impl VirtAddr {
    pub const ZERO: Self = Self(0);

    #[must_use]
    pub const fn raw(&self) -> usize {
        self.0
    }

    #[must_use]
    pub const fn align_up(&self, page_size: usize) -> Self {
        debug_assert!(page_size.is_power_of_two());
        Self(helper::align_up(self.0, page_size))
    }

    #[must_use]
    pub const fn align_down(&self, page_size: usize) -> Self {
        debug_assert!(page_size.is_power_of_two());
        Self(helper::align_down(self.0, page_size))
    }

    #[must_use]
    pub const fn difference(&self, other: VirtAddr) -> isize {
        self.0 as isize - other.0 as isize
    }

    #[must_use]
    pub const fn new(addr: usize) -> Self {
        Self(addr)
    }

    #[must_use]
    pub const fn offset_by(&self, amount: isize) -> Option<Self> {
        let res = if amount < 0 {
            self.0.checked_sub(amount.unsigned_abs())
        } else {
            self.0.checked_add(amount as usize)
        };

        match res {
            Some(res) => Some(Self(res)),
            None => None,
        }
    }
}

impl From<usize> for VirtAddr {
    fn from(value: usize) -> Self {
        Self(value)
    }
}

#[cfg(feature = "riscv-sbi")]
impl From<VirtAddr> for crate::arch::mmu::VirtualAddress {
    fn from(value: VirtAddr) -> Self {
        unsafe { crate::arch::mmu::VirtualAddress::from_raw_unchecked(value.0) }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct KernelVirtAddr(VirtAddr);

impl KernelVirtAddr {
    pub const NULL: Self = Self(VirtAddr(0));

    pub fn new<A: Into<VirtAddr>>(addr: A) -> Result<Self, Error> {
        // TODO(aeryz): only accept if its a direct map?
        // How about kernel text? Maybe this should better be able to represent all high
        // base memory
        Ok(Self(addr.into()))
    }

    pub fn offset_by(&self, amount: isize) -> Option<Self> {
        self.0
            .offset_by(amount)
            .map(Self::new)
            .transpose()
            .unwrap_or(None)
    }

    /// Returns error if the address is not aligned for `T`.
    #[must_use]
    pub const fn as_ptr<T>(&self) -> Result<*const T, Error> {
        if self.check_aligned::<T>() {
            Ok(self.raw() as *const T)
        } else {
            Err(Error::Unaligned)
        }
    }

    // TODO(aeryz): for performance reasons, should this be a debug assertion
    // instead? And then we can push the alignment check to `new`. Because a
    // `KernelVirtAddr` should not be used for pointing multiple things. We can also
    // restrict `offset_by` to follow an alignment to guarantee the validity of a
    // ptr. Hmmm then we might as well just store a `PhantomData<T>` and make
    // `offset_by` take a count of `T`.
    /// Returns error if the address is not aligned for `T`.
    #[must_use]
    pub const fn as_ptr_mut<T>(&self) -> Result<*mut T, Error> {
        if self.check_aligned::<T>() {
            Ok(self.raw() as *mut T)
        } else {
            Err(Error::Unaligned)
        }
    }

    #[must_use]
    pub const fn check_aligned<T>(&self) -> bool {
        self.raw() % core::mem::align_of::<T>() == 0
    }

    #[must_use]
    pub const fn raw(&self) -> usize {
        self.0.0
    }
}

impl From<KernelVirtAddr> for VirtAddr {
    fn from(value: KernelVirtAddr) -> Self {
        value.0
    }
}

pub struct PhysAddr(usize);
