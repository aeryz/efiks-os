use alloc::boxed::Box;

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
    Other(Box<dyn IntoError>),
}

impl<T: IntoError + 'static> From<T> for Error {
    fn from(value: T) -> Self {
        Self::Other(Box::new(value))
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
            Error::Other(err) => err.to_errno(),
        }
    }
}
