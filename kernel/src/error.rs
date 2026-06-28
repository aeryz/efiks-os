use efiks_types::{Errno, IntoError};

#[derive(Debug)]
pub enum Error {
    Unaligned,
    Overflow,
    Oom,
    NotFound,
    InvalidArgs,
    Unmapped,
    NoSys,
    Errno(Errno),
    Other(&'static dyn IntoError),
}

impl<T: IntoError> From<T> for Error {
    fn from(value: T) -> Self {
        Self::Errno(value.to_errno())
    }
}

impl From<Error> for Errno {
    fn from(value: Error) -> Self {
        match value {
            Error::Unaligned => Self::EInval,
            Error::Overflow => Self::EOverflow,
            Error::Oom => Self::ENoMem,
            Error::NotFound => Self::ENoEnt,
            Error::InvalidArgs => Self::EInval,
            Error::Unmapped => Self::EFault,
            Error::NoSys => Self::ENoSys,
            Error::Errno(errno) => errno,
            Error::Other(err) => err.to_errno(),
        }
    }
}
