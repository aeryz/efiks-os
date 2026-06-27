#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct PhysAddr(usize);

impl PhysAddr {
    pub const ZERO: Self = Self(0);

    #[must_use]
    pub const fn raw(&self) -> usize {
        self.0
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

#[cfg(feature = "riscv-sbi")]
impl From<PhysAddr> for crate::arch::mmu::PhysicalAddress {
    fn from(value: PhysAddr) -> Self {
        unsafe { crate::arch::mmu::PhysicalAddress::from_raw_unchecked(value.0) }
    }
}

#[cfg(feature = "riscv-sbi")]
impl From<crate::arch::mmu::PhysicalAddress> for PhysAddr {
    fn from(value: crate::arch::mmu::PhysicalAddress) -> Self {
        PhysAddr::new(value.raw())
    }
}
