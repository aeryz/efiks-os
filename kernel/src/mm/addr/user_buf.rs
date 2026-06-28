use crate::mm::VirtAddr;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct UserBuf(VirtAddr);

impl UserBuf {
    #[must_use]
    pub const fn new(addr: usize) -> Option<Self> {
        if addr != 0 {
            Some(Self(VirtAddr::new(addr)))
        } else {
            None
        }
    }

    pub unsafe fn copy_from_user(&self, dest: &mut [u8]) {
        let _ = unsafe { self.copy_from_user_until(dest, |_| false) };
    }

    pub unsafe fn copy_from_user_until(
        &self,
        dest: &mut [u8],
        should_stop: fn(u8) -> bool,
    ) -> Option<usize> {
        for i in 0..dest.len() {
            unsafe {
                let b = *((self.0.raw() + i) as *const u8);
                if should_stop(b) {
                    return Some(i);
                }
                dest[i] = b;
            }
        }

        None
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct UserBufMut(UserBuf);

impl UserBufMut {
    #[must_use]
    pub const fn new(addr: usize) -> Option<Self> {
        match UserBuf::new(addr) {
            Some(u) => Some(Self(u)),
            None => None,
        }
    }

    #[allow(unused)]
    pub unsafe fn copy_from_user(&self, dest: &mut [u8]) {
        unsafe {
            self.0.copy_from_user(dest);
        }
    }

    #[allow(unused)]
    pub unsafe fn copy_from_user_until(
        &self,
        dest: &mut [u8],
        should_stop: fn(u8) -> bool,
    ) -> Option<usize> {
        unsafe { self.0.copy_from_user_until(dest, should_stop) }
    }

    pub unsafe fn copy_into_user(&self, src: &[u8]) {
        for (i, b) in src.iter().enumerate() {
            unsafe {
                *((self.0.0.raw() + i) as *mut u8) = *b;
            }
        }
    }
}
