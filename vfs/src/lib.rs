//! Shared VFS interfaces.
//!
//! This crate defines the contracts between the kernel VFS layer, concrete
//! filesystems, and block devices. It deliberately does not own global mount
//! state or any concrete caching policy. Filesystems expose [`VNode`] objects
//! for path traversal and file I/O, while the kernel is responsible for routing
//! absolute paths to the mounted [`Filesystem`] that should handle them.
//!
//! The current block-device API is type-based: reads and writes are associated
//! functions on [`BlockDevice`] rather than methods on a device instance. That
//! keeps early boot and generic filesystem mounting simple, but it means a
//! driver type represents one logical backing device.

#![no_std]

use alloc::sync::Arc;

extern crate alloc;

/// Size of the smallest block-device transfer supported by the VFS traits.
pub const SECTOR_SIZE: usize = 512;

/// Result type used by VFS, filesystem, and block-device operations.
pub type VfsResult<T> = Result<T, VfsError>;

/// Error values shared across the VFS boundary.
///
/// The enum is intentionally small while the filesystem layer is still young.
/// Concrete implementations should use [`VfsError::Fs`] for malformed on-disk
/// state or unsupported filesystem operations until more specific variants are
/// introduced.
#[derive(Debug, Copy, Clone, PartialEq)]
pub enum VfsError {
    /// The underlying block device failed to complete a sector transfer.
    DeviceIO,
    /// Filesystem-specific failure.
    Fs,
    /// A mount already exists at the requested mount path.
    AlreadyMounted,
    /// A file offset or seek target is outside the supported range.
    OutOfBounds,
    /// Catch-all for errors that do not yet have a precise variant.
    Unknown,
}

/// A filesystem node exposed through the VFS.
///
/// A [`VNode`] is usually backed by an inode in the concrete filesystem, but
/// the VFS only requires path traversal plus byte-oriented read/write
/// operations. Implementations are responsible for their own internal
/// synchronization.
pub trait VNode: Send + Sync {
    /// Opens a file at `path` relative to the `self` inode. Meaning,
    /// if one has the inode for `/path/to/folder`, opening `file/path` will
    /// open the file at `/path/to/folder/file/path`.
    fn open(&self, path: &[u8]) -> VfsResult<File>;

    /// Reads at most `buf.len()` bytes from this inode starting from the offset
    /// and returns the number of bytes read.
    fn read(&self, offset: usize, buf: &mut [u8]) -> VfsResult<usize>;

    /// Writes at most `buf.len()` bytes into this inode starting from the
    /// `offset` and returns the number of bytes written.
    fn write(&self, offset: usize, buf: &[u8]) -> VfsResult<usize>;

    /// Returns the file size in bytes.
    fn sz(&self) -> usize;
}

/// A mounted filesystem instance.
///
/// The kernel mount table stores trait objects of this type. The VFS layer
/// should not place a coarse lock around the entire filesystem; concrete
/// filesystems should protect their own mutable metadata and caches.
pub trait Filesystem: Send + Sync {
    /// Returns the root node for this filesystem.
    fn root(&self) -> VfsResult<Arc<dyn VNode>>;
}

/// Sector-oriented block-device interface used by filesystems.
///
/// A future cache layer can implement this trait by wrapping a real block
/// device type, allowing filesystems to use cached reads/writes without knowing
/// whether the backing device is cached.
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

/// An open file description.
///
/// The [`File`] owns the current byte offset while the underlying [`VNode`]
/// owns access to the file data. Cloning the vnode into multiple [`File`]
/// objects would give each open file its own offset.
pub struct File {
    /// Filesystem node backing this open file.
    pub inode: Arc<dyn VNode>,
    /// Current byte offset used by [`File::read`] and [`File::write`].
    pub offset: usize,
}

/// Seek base used by [`File::seek`].
pub enum SeekFrom {
    /// Set the offset relative to the start of the file.
    Start(usize),
    /// Set the offset relative to the current offset.
    Current(isize),
    /// Set the offset relative to the end of the file.
    End(isize),
}

impl File {
    /// Reads from the current offset and advances it by the number of bytes
    /// read.
    pub fn read(&mut self, buf: &mut [u8]) -> VfsResult<usize> {
        let n_read = self.inode.read(self.offset, buf)?;
        self.offset += n_read;
        Ok(n_read)
    }

    /// Writes at the current offset and advances it by the number of bytes
    /// written.
    pub fn write(&mut self, buf: &[u8]) -> VfsResult<usize> {
        let n_written = self.inode.write(self.offset, buf)?;
        self.offset += n_written;
        Ok(n_written)
    }

    /// Updates the current file offset.
    ///
    /// Seeking to or beyond the current file size is rejected for now because
    /// sparse files and file growth are not implemented.
    pub fn seek(&mut self, offset: SeekFrom) -> VfsResult<()> {
        let file_size = self.inode.sz();
        let checked_offset = |start: usize, offset: isize| {
            if offset >= 0 {
                start.checked_add(offset as usize)
            } else {
                start.checked_sub(offset.unsigned_abs())
            }
        };
        let new_offset = match offset {
            SeekFrom::Start(offset) => Some(offset),
            SeekFrom::Current(offset) => checked_offset(self.offset, offset),
            SeekFrom::End(offset) => checked_offset(file_size, offset),
        }
        .ok_or(VfsError::OutOfBounds)?;

        if new_offset > self.inode.sz() {
            return Err(VfsError::OutOfBounds);
        }

        self.offset = new_offset;
        Ok(())
    }
}
