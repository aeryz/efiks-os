use crate::mm::VirtAddr;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct UserPtr(VirtAddr);

impl UserPtr {
    pub const NULL: Self = Self(VirtAddr::new(0));

    #[must_use]
    pub const fn new(addr: usize) -> Self {
        Self(VirtAddr::new(addr))
    }

    #[must_use]
    pub const fn offset_by(&self, amount: isize) -> Option<Self> {
        match self.0.offset_by(amount) {
            Some(v) => Some(Self(v)),
            None => None,
        }
    }

    #[must_use]
    pub fn is_null(&self) -> bool {
        *self == Self::NULL
    }

    #[must_use]
    pub const fn raw(&self) -> usize {
        self.0.raw()
    }
}
