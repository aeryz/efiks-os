#![no_std]

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[repr(isize)]
pub enum Errno {
    /// No such file or directory
    ENoEnt = 2,
    /// I/O error
    EIO = 5,
    /// Exec format error
    ENoExec = 8,
    /// Cannot allocate memory
    ENoMem = 12,
    /// Bad address
    EFault = 14,
    /// Device or resource busy
    EBusy = 16,
    /// Function not implemented
    ENoSys = 38,
    /// Invalid argument
    EInval = 22,
    /// Value too large for defined data type
    EOverflow = 75,
}

pub trait IntoError: core::fmt::Debug {
    fn to_errno(&self) -> Errno;
}
