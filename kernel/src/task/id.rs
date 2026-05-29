use core::{
    fmt::Debug,
    sync::atomic::{AtomicUsize, Ordering},
};

static PID_COUNTER: AtomicUsize = AtomicUsize::new(0);

#[derive(Copy, Clone, PartialOrd, Ord, PartialEq, Eq)]
#[repr(C)]
pub struct Pid(usize);

impl Pid {
    #[must_use]
    pub fn create_next() -> Self {
        Pid(PID_COUNTER.fetch_add(1, Ordering::Acquire))
    }

    #[allow(unused)]
    pub const fn raw(&self) -> usize {
        self.0
    }
}

impl Debug for Pid {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_tuple("Pid").field(&self.0).finish()
    }
}
