#![no_std]

mod lib2;

use alloc::sync::Arc;
use bitflags::bitflags;
use ksync::{RwLock, SpinLock};

extern crate alloc;

/// Initially support `SECTOR_SIZE` of 512 bytes
pub const SECTOR_SIZE: usize = 512;

pub type VfsResult<T> = Result<T, VfsError>;

pub enum VfsError {
    DeviceIO,
    Fs,
}

///
pub trait VNode: Sized {
    fn open(&self, path: &[u8]) -> VfsResult<File<Self>>;
}

pub trait Filesystem: Sized {
    type VNode: VNode;

    /// Initializes the filesystem to be mounted at '/'.
    /// Similar to how mount works in Linux but we prefer to use a
    /// different naming at this time not to confuse the reader into
    /// thinking we support multiple mount points or you can mount into
    /// a path other than '/'.
    ///
    /// Constructs and returns the self to be used by the OS.
    /// Returns `VfsError::FsInitError` on initialization error.
    fn initialize() -> VfsResult<Arc<SpinLock<Self>>>;

    /// Returns the root inode
    fn root(fs: Arc<SpinLock<Self>>) -> VfsResult<Self::VNode>;
}

pub trait BlockDevice {
    /// Reads [`SECTOR_SIZE`] bytes from `sector` into `buf`
    ///
    /// Returns `VfsError::DeviceIO` on error
    fn read_sector(sector: usize, buf: &mut [u8; SECTOR_SIZE]) -> VfsResult<()>;

    /// Writes [`SECTOR_SIZE`] bytes from `buf` into `sector`
    ///
    /// Returns `VfsError::DeviceIO` on error
    fn write_sector(sector: usize, buf: &[u8; SECTOR_SIZE]) -> VfsResult<()>;
}

pub struct File<N: VNode> {
    pub inode: Arc<N>,
    pub offset: usize,
}
