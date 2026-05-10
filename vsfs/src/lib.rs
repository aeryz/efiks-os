//! Very Simple File System implementation.
//!
//! VSFS is the first concrete filesystem used by the kernel VFS. It is small on
//! purpose: a superblock identifies the filesystem layout, an inode table
//! stores fixed-size inode records, and directories are files containing
//! fixed-size directory entries.
//!
//! Synchronization is split by responsibility. The filesystem object owns a
//! short-held inode cache lock, while each cached inode owns an [`RwLock`] for
//! its mutable inode metadata. This lets independent inodes be accessed in
//! parallel and keeps the VFS mount layer from serializing an entire
//! filesystem. Raw sector caching is intentionally left below this crate; a
//! cached block device can implement [`BlockDevice`] and be mounted under VSFS.

#![no_std]

use core::{marker::PhantomData, ptr};

use alloc::{
    collections::btree_map::{BTreeMap, Entry},
    sync::Arc,
};
use derivative::Derivative;
use ksync::{ReadLockGuard, RwLock, SpinLock};
use vfs::{BlockDevice, File, Filesystem, SECTOR_SIZE, VNode, VfsError, VfsResult};

extern crate alloc;

const MAGIC: u32 = 0x5653_4653; // "VSFS"
const MAX_DIRENTS_IN_SECTOR: usize = 512 / size_of::<DirEnt>();
const BLOCK_SIZE: usize = 4096;
const SECTORS_PER_BLOCK: usize = BLOCK_SIZE / SECTOR_SIZE;

/// Mounted VSFS instance.
///
/// The superblock is immutable after mount. The inode cache maps VSFS inode
/// numbers to shared in-memory inode objects; it is filesystem-specific because
/// the VFS does not know how VSFS inode numbers map to on-disk metadata.
pub struct Vsfs<BD: BlockDevice> {
    superblock: SuperBlock,
    inode_cache: SpinLock<BTreeMap<usize, Arc<INode<BD>>>>,
    _marker: PhantomData<BD>,
}

/// VSFS-specific error marker.
///
/// Currently the public API reports errors through [`VfsError`], so this type
/// is reserved for a future split between generic VFS errors and detailed VSFS
/// errors.
pub enum Error {}

/// Cached VSFS inode.
///
/// The inode number and owning filesystem are immutable. The on-disk inode
/// payload lives behind an [`RwLock`] so multiple readers can inspect file
/// metadata concurrently while writes take exclusive access.
#[derive(Derivative)]
#[derivative(Clone(bound = ""))]
pub struct INode<BD: BlockDevice> {
    inum: usize,
    fs: Arc<Vsfs<BD>>,
    inner: Arc<RwLock<INodeInner>>,
    _marker: PhantomData<BD>,
}

/// On-disk inode payload.
///
/// This structure is read directly from the inode table, so the representation
/// must stay compatible with the image creation tool and existing disk images.
#[derive(Clone, Copy)]
#[repr(C)]
pub struct INodeInner {
    /// File kind.
    pub ty: Type,
    /// Number of directory entries pointing at this inode.
    pub link_count: u16,
    /// Basic file metadata.
    pub metadata: Metadata,
    /// Direct data block numbers.
    pub direct_blocks: [u32; 12],
    /// Indirect block number. Not used by the current implementation.
    pub indirect_block: u32,
}

/// VSFS inode type stored on disk.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[repr(u16)]
pub enum Type {
    /// Directory containing [`DirEnt`] records.
    Directory = 1,
    /// Regular file.
    File = 2,
}

/// Basic inode metadata stored on disk.
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct Metadata {
    /// ID of the device containing file
    // TODO(aeryz): should I have a dev_t?
    pub dev: u32,
    /// Total size of the file in bytes
    pub sz: u32,
}

/// Fixed-size directory entry stored in directory data blocks.
///
/// Names are byte strings, not UTF-8 strings. Only the first
/// [`DirEnt::name_len`] bytes in [`DirEnt::name`] are part of the entry name.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct DirEnt {
    /// Target inode number.
    pub inum: u32,
    /// Number of valid bytes in [`DirEnt::name`].
    pub name_len: u8,
    /// Inline file name storage.
    pub name: [u8; 27],
}

impl<BD: BlockDevice> INode<BD> {
    /// Looks up one path component inside a directory inode.
    ///
    /// `path` must be a single component without `/`. The caller passes the
    /// directory inode read guard so the directory metadata remains stable
    /// while the directory entries are scanned.
    fn lookup_path(
        fs: &Arc<Vsfs<BD>>,
        inode: ReadLockGuard<'_, INodeInner>,
        path: &[u8],
    ) -> VfsResult<Arc<Self>> {
        if inode.ty != Type::Directory {
            return Err(VfsError::Fs);
        }

        let n_dirent_in_node = inode.metadata.sz as usize / size_of::<DirEnt>();

        let buf = &mut [0; 512];
        for (block_idx, block) in inode.direct_blocks.iter().enumerate() {
            let mut sector = block * 4096 / 512;

            for sector_idx in 0..8 {
                BD::read_sector(sector as usize, buf)?;

                for cur_dir_idx in 0..MAX_DIRENTS_IN_SECTOR {
                    if (block_idx * 8 + sector_idx) * MAX_DIRENTS_IN_SECTOR + cur_dir_idx
                        >= n_dirent_in_node
                    {
                        return Err(VfsError::Fs);
                    }

                    let dirent = unsafe {
                        ptr::read_unaligned(
                            buf[(size_of::<DirEnt>() * cur_dir_idx)..].as_ptr() as *const DirEnt
                        )
                    };
                    let name_len = dirent.name_len as usize;
                    if name_len > dirent.name.len() {
                        return Err(VfsError::Fs);
                    }

                    if &dirent.name[0..name_len] == path {
                        return Ok(Vsfs::<BD>::read_inode(fs.clone(), dirent.inum as usize)?);
                    }
                }

                sector += 1;
            }
        }

        Err(VfsError::Fs)
    }
}

impl<BD: BlockDevice + 'static + Send + Sync> VNode for INode<BD> {
    /// Resolves a relative path from this inode and returns an open file.
    ///
    /// Empty components are ignored, so repeated slashes behave like a single
    /// separator. Opening through a regular file is rejected.
    fn open(&self, path: &[u8]) -> VfsResult<File> {
        let mut current = Arc::new(self.clone());
        for path in path.split(|b| *b == b'/').filter(|p| !p.is_empty()) {
            let inode = current.inner.read_lock();
            if inode.ty == Type::File {
                return Err(VfsError::Fs);
            }
            let next_inode = Self::lookup_path(&self.fs, inode, path)?;

            if current.inum != next_inode.inum {
                current = next_inode;
            }
        }

        Ok(File {
            inode: current,
            offset: 0,
        })
    }

    /// Reads bytes from a regular file.
    ///
    /// VSFS currently supports only direct blocks. Reading past the end returns
    /// `Ok(0)`, while discovering malformed block metadata returns
    /// [`VfsError::Fs`] unless some bytes have already been read.
    fn read(&self, mut offset: usize, buf: &mut [u8]) -> VfsResult<usize> {
        let inner = self.inner.read_lock();
        if inner.ty != Type::File {
            return Err(VfsError::Fs);
        }

        let file_size = inner.metadata.sz as usize;
        if offset >= file_size || buf.is_empty() {
            return Ok(0);
        }

        let mut total_read = 0;
        let mut sector_buf = [0; SECTOR_SIZE];
        let mut remaining = core::cmp::min(buf.len(), file_size - offset);

        while remaining > 0 {
            let logical_block = offset / BLOCK_SIZE;
            if logical_block >= inner.direct_blocks.len() {
                return if total_read > 0 {
                    Ok(total_read)
                } else {
                    Err(VfsError::Fs)
                };
            }

            let block = inner.direct_blocks[logical_block];
            if block == 0 {
                return if total_read > 0 {
                    Ok(total_read)
                } else {
                    Err(VfsError::Fs)
                };
            }

            let block_offset = offset % BLOCK_SIZE;
            let sector_in_block = block_offset / SECTOR_SIZE;
            let sector_offset = block_offset % SECTOR_SIZE;
            let sector = block as usize * SECTORS_PER_BLOCK + sector_in_block;

            BD::read_sector(sector, &mut sector_buf)?;

            let readable_from_sector = SECTOR_SIZE - sector_offset;
            let to_copy = core::cmp::min(readable_from_sector, remaining);
            buf[total_read..total_read + to_copy]
                .copy_from_slice(&sector_buf[sector_offset..sector_offset + to_copy]);

            offset += to_copy;
            total_read += to_copy;
            remaining -= to_copy;
        }

        Ok(total_read)
    }

    /// Writes bytes to a regular file.
    ///
    /// This implementation writes only within the existing file size. It does
    /// not allocate new blocks, grow files, or update timestamps.
    fn write(&self, mut offset: usize, buf: &[u8]) -> VfsResult<usize> {
        let inner = self.inner.write_lock();
        if inner.ty != Type::File {
            return Err(VfsError::Fs);
        }

        let file_size = inner.metadata.sz as usize;
        if offset >= file_size || buf.is_empty() {
            return Ok(0);
        }

        let mut total_written = 0;
        let mut sector_buf = [0; SECTOR_SIZE];
        let mut remaining = core::cmp::min(buf.len(), file_size - offset);

        while remaining > 0 {
            let logical_block = offset / BLOCK_SIZE;
            if logical_block >= inner.direct_blocks.len() {
                return if total_written > 0 {
                    Ok(total_written)
                } else {
                    Err(VfsError::Fs)
                };
            }

            let block = inner.direct_blocks[logical_block];
            if block == 0 {
                return if total_written > 0 {
                    Ok(total_written)
                } else {
                    Err(VfsError::Fs)
                };
            }

            let block_offset = offset % BLOCK_SIZE;
            let sector_in_block = block_offset / SECTOR_SIZE;
            let sector_offset = block_offset % SECTOR_SIZE;
            let sector = block as usize * SECTORS_PER_BLOCK + sector_in_block;

            let writable_to_sector = SECTOR_SIZE - sector_offset;
            let to_copy = core::cmp::min(writable_to_sector, remaining);

            if to_copy != SECTOR_SIZE {
                BD::read_sector(sector, &mut sector_buf)?;
            }

            sector_buf[sector_offset..sector_offset + to_copy]
                .copy_from_slice(&buf[total_written..total_written + to_copy]);

            BD::write_sector(sector, &sector_buf)?;

            offset += to_copy;
            total_written += to_copy;
            remaining -= to_copy;
        }
        Ok(total_written)
    }

    /// Returns the current file size recorded in the inode.
    fn sz(&self) -> usize {
        self.inner.read_lock().metadata.sz as usize
    }
}

/// VSFS superblock stored at sector 0.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
struct SuperBlock {
    magic: u32,
    nblocks: u32,
    ninodes: u32,
    inode_bitmap_block: u32,
    data_bitmap_block: u32,
    inode_table_start: u32,
    inode_table_blocks: u32,
    data_block_start: u32,
}

/// Mounts a VSFS image from the given block device type.
///
/// Initialization reads and validates the superblock, creates the filesystem
/// object, and warms the inode cache with the root inode.
pub fn initialize<BD: BlockDevice>() -> VfsResult<Arc<Vsfs<BD>>> {
    let buf = &mut [0; 512];
    BD::read_sector(0, buf)?;

    let sb = unsafe { ptr::read_unaligned(buf.as_ptr() as *const SuperBlock) };

    if sb.magic != MAGIC {
        return Err(VfsError::Fs);
    }

    let vsfs = Arc::new(Vsfs {
        superblock: sb,
        inode_cache: SpinLock::new(BTreeMap::new()),
        _marker: PhantomData,
    });

    // Read will force the root inode to be cached
    let _ = Vsfs::<BD>::read_inode(vsfs.clone(), 1)?;

    Ok(vsfs)
}

impl<BD: BlockDevice + 'static + Send + Sync> Filesystem for Vsfs<BD> {
    /// Returns the cached root inode.
    fn root(&self) -> VfsResult<Arc<dyn VNode>> {
        Ok(self
            .inode_cache
            .lock()
            .get(&1)
            .expect("root inode always exists")
            .clone())
    }
}

impl<BD: BlockDevice> Vsfs<BD> {
    /// Returns a cached inode, reading it from disk on cache miss.
    ///
    /// The inode cache lock is not held while the disk is accessed. The cache
    /// is checked first, then a missed inode is read and inserted.
    fn read_inode(fs: Arc<Self>, inum: usize) -> VfsResult<Arc<INode<BD>>> {
        let inode_table_start = fs.superblock.inode_table_start;
        let mut cache = fs.inode_cache.lock();

        match cache.entry(inum) {
            Entry::Vacant(_) => {
                // Dropping the lock to unblock the cache while doing physical IO
                drop(cache);
                let i = Arc::new(INode {
                    inum,
                    fs: fs.clone(),
                    inner: Arc::new(RwLock::new(Self::read_inode_from_block(
                        inode_table_start as usize,
                        inum,
                    )?)),
                    _marker: PhantomData,
                });
                // Again checking the existence of the value so that we only insert once in case
                // there are multiple threads here racing to add the same thing. If we were to
                // blindly `insert` here, we would have inserted twice and it would break the
                // rule of "1 reference counting per inode".
                match fs.inode_cache.lock().entry(inum) {
                    Entry::Vacant(inode) => {
                        inode.insert(i.clone());
                        Ok(i)
                    }
                    Entry::Occupied(inode) => Ok(inode.get().clone()),
                }
            }
            Entry::Occupied(occupied_entry) => Ok(occupied_entry.get().clone()),
        }
    }

    /// Reads one inode payload from the on-disk inode table.
    fn read_inode_from_block(inode_table_start: usize, inum: usize) -> VfsResult<INodeInner> {
        let inode_byte_offset = inode_table_start * 4096 + inum * size_of::<INodeInner>();
        let inode_sector = inode_byte_offset / 512;
        let inode_offset = inode_byte_offset % 512;

        let buf = &mut [0; 512];

        BD::read_sector(inode_sector, buf)?;

        let inner =
            unsafe { ptr::read_unaligned(buf[inode_offset..].as_ptr() as *const INodeInner) };

        Ok(inner)
    }
}
