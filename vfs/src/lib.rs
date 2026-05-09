#![no_std]

use alloc::sync::Arc;

extern crate alloc;

/// Initially support `SECTOR_SIZE` of 512 bytes
pub const SECTOR_SIZE: usize = 512;

pub type VfsResult<T> = Result<T, VfsError>;

#[derive(Debug, Copy, Clone, PartialEq)]
pub enum VfsError {
    DeviceIO,
    Fs,
    AlreadyMounted,
    Unknown,
}

pub trait VNode: Send + Sync {
    /// Opens a file at `path` relative to the `self` inode. Meaning,
    /// if one has the inode for `/path/to/folder`, opening `file/path` will
    /// open the file at `/path/to/folder/file/path`.
    fn open(&self, path: &[u8]) -> VfsResult<File>;

    /// Reads at most `buf.len()` bytes from this inode starting from the offset
    /// and returns the number of bytes read.
    fn read(&self, offset: usize, buf: &mut [u8]) -> VfsResult<usize>;
}

pub trait Filesystem: Send + Sync {
    /// Returns the root inode
    fn root(&self) -> VfsResult<Arc<dyn VNode>>;
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

pub struct File {
    pub inode: Arc<dyn VNode>,
    pub offset: usize,
}

impl File {
    pub fn read(&mut self, buf: &mut [u8]) -> VfsResult<usize> {
        let n_read = self.inode.read(self.offset, buf)?;
        self.offset += n_read;
        Ok(n_read)
    }
}
