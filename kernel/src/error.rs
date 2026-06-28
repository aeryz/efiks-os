use crate::{errno::Errno, exec};

#[derive(Debug)]
pub enum Error {
    Vfs(vfs::VfsError),
    Elf(exec::elf::Error),
    Unaligned,
    Overflow,
    Oom,
    NotFound,
    InvalidArgs,
    Unmapped,
    NoSys,
}

impl From<vfs::VfsError> for Error {
    fn from(value: vfs::VfsError) -> Self {
        Self::Vfs(value)
    }
}

impl From<exec::elf::Error> for Error {
    fn from(value: exec::elf::Error) -> Self {
        Self::Elf(value)
    }
}

impl From<Error> for Errno {
    fn from(value: Error) -> Self {
        match value {
            Error::Vfs(_) => Self::EIO,
            Error::Elf(_) => Self::ENoExec,
            Error::Unaligned => Self::EInval,
            Error::Overflow => Self::EOverflow,
            Error::Oom => Self::ENoMem,
            Error::NotFound => Self::ENoEnt,
            Error::InvalidArgs => Self::EInval,
            Error::Unmapped => Self::EFault,
            Error::NoSys => Self::ENoSys,
        }
    }
}
