use crate::helper;

#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct VirtAddr(usize);

impl VirtAddr {
    pub const ZERO: Self = Self(0);

    #[must_use]
    pub const fn new(addr: usize) -> Self {
        Self(addr)
    }

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
