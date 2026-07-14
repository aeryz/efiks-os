#![no_std]

pub mod syscalls;

#[inline(always)]
pub fn write<T: AsRef<[u8]>>(buf: T) -> isize {
    syscalls::write(buf.as_ref().as_ptr(), buf.as_ref().len())
}

#[inline(always)]
pub fn open(path: *const u8, flags: u32) -> isize {
    syscalls::open(path, flags)
}
