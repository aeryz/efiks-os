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

use core::{marker::PhantomData, ptr, slice};

use alloc::{
    collections::btree_map::{BTreeMap, Entry},
    sync::Arc,
};
use derivative::Derivative;
use ksync::{ReadLockGuard, RwLock, SpinLock};
use vfs::{BlockDevice, File, Filesystem, SECTOR_SIZE, VNode, VfsError, VfsResult};

extern crate alloc;

const MAGIC: u32 = 0x5653_4653; // "VSFS"
const BLOCK_SIZE: usize = 4096;
const SECTORS_PER_BLOCK: usize = BLOCK_SIZE / SECTOR_SIZE;
const DIRECT_BLOCKS: usize = 12;
const INDIRECT_BLOCK_ENTRIES: usize = BLOCK_SIZE / size_of::<u32>();

/// Mounted VSFS instance.
///
/// The superblock is immutable after mount. The inode cache maps VSFS inode
/// numbers to shared in-memory inode objects; it is filesystem-specific because
/// the VFS does not know how VSFS inode numbers map to on-disk metadata.
pub struct Vsfs<BD: BlockDevice> {
    superblock: SuperBlock,
    inode_cache: SpinLock<BTreeMap<usize, Arc<INode<BD>>>>,
    inode_bitmap_lock: SpinLock<()>,
    data_bitmap_lock: SpinLock<()>,
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
    pub direct_blocks: [u32; DIRECT_BLOCKS],
    /// Block containing additional data block numbers.
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
    /// Resolves a file-relative block number through the inode's direct or
    /// single-indirect block pointers.
    fn data_block(inode: &INodeInner, logical_block: usize) -> VfsResult<u32> {
        let block = if logical_block < DIRECT_BLOCKS {
            inode.direct_blocks[logical_block]
        } else {
            let indirect_index = logical_block - DIRECT_BLOCKS;
            if indirect_index >= INDIRECT_BLOCK_ENTRIES || inode.indirect_block == 0 {
                return Err(VfsError::Fs);
            }

            let pointer_offset = indirect_index * size_of::<u32>();
            let sector_in_block = pointer_offset / SECTOR_SIZE;
            let sector_offset = pointer_offset % SECTOR_SIZE;
            let sector = inode.indirect_block as usize * SECTORS_PER_BLOCK + sector_in_block;
            let mut sector_buf = [0; SECTOR_SIZE];
            BD::read_sector(sector, &mut sector_buf)?;

            u32::from_ne_bytes([
                sector_buf[sector_offset],
                sector_buf[sector_offset + 1],
                sector_buf[sector_offset + 2],
                sector_buf[sector_offset + 3],
            ])
        };

        if block == 0 {
            return Err(VfsError::Fs);
        }
        Ok(block)
    }

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
        Self::lookup_path_inner(fs, &inode, path)
    }

    fn lookup_path_inner(
        fs: &Arc<Vsfs<BD>>,
        inode: &INodeInner,
        path: &[u8],
    ) -> VfsResult<Arc<Self>> {
        if inode.ty != Type::Directory {
            return Err(VfsError::Fs);
        }

        if inode.metadata.sz as usize % size_of::<DirEnt>() != 0 {
            return Err(VfsError::Fs);
        }
        let buf = &mut [0; 512];
        for dirent_idx in 0..inode.metadata.sz as usize / size_of::<DirEnt>() {
            let dirent_offset = dirent_idx * size_of::<DirEnt>();
            let block_idx = dirent_offset / BLOCK_SIZE;
            let block_offset = dirent_offset % BLOCK_SIZE;
            let block = Self::data_block(inode, block_idx)?;

            let sector = block as usize * SECTORS_PER_BLOCK + block_offset / SECTOR_SIZE;
            BD::read_sector(sector, buf)?;

            let dirent = unsafe {
                ptr::read_unaligned(buf[block_offset % SECTOR_SIZE..].as_ptr() as *const DirEnt)
            };
            let name_len = dirent.name_len as usize;
            if name_len > dirent.name.len() {
                return Err(VfsError::Fs);
            }

            if &dirent.name[0..name_len] == path {
                return Ok(Vsfs::<BD>::read_inode(fs.clone(), dirent.inum as usize)?);
            }
        }

        Err(VfsError::NotFound)
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

    fn create(&self, path: &[u8]) -> VfsResult<File> {
        let mut current = Arc::new(self.clone());
        let mut err: Option<VfsError> = None;
        let mut leaf = None;
        let mut components = path
            .split(|b| *b == b'/')
            .filter(|p| !p.is_empty())
            .peekable();
        while let Some(path) = components.next() {
            if let Some(err) = err {
                return Err(err);
            }

            let is_leaf = components.peek().is_none();
            let inode = current.inner.read_lock();
            if inode.ty == Type::File {
                return Err(VfsError::Fs);
            }

            match Self::lookup_path(&self.fs, inode, path) {
                Ok(next_inode) => {
                    if current.inum != next_inode.inum {
                        current = next_inode;
                    }
                }
                Err(e) => {
                    if e == VfsError::NotFound && is_leaf {
                        leaf = Some(path);
                    }
                    err = Some(e);
                }
            }
        }

        if let Some(VfsError::NotFound) = err {
            let leaf = leaf.ok_or(VfsError::Fs)?;
            let inode = self.fs.create_file(&current, leaf)?;
            return Ok(File { inode, offset: 0 });
        }

        Err(VfsError::Fs)
    }

    /// Reads bytes from a regular file.
    ///
    /// Reading past the end returns `Ok(0)`, while discovering malformed block
    /// metadata returns [`VfsError::Fs`] unless some bytes have already been
    /// read.
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
        let mut mapped_block = None;

        while remaining > 0 {
            let logical_block = offset / BLOCK_SIZE;
            let block = match mapped_block {
                Some((mapped_logical_block, block)) if mapped_logical_block == logical_block => {
                    block
                }
                _ => match Self::data_block(&inner, logical_block) {
                    Ok(block) => {
                        mapped_block = Some((logical_block, block));
                        block
                    }
                    Err(err) => {
                        return if total_read > 0 {
                            Ok(total_read)
                        } else {
                            Err(err)
                        };
                    }
                },
            };

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
        let mut mapped_block = None;

        while remaining > 0 {
            let logical_block = offset / BLOCK_SIZE;
            let block = match mapped_block {
                Some((mapped_logical_block, block)) if mapped_logical_block == logical_block => {
                    block
                }
                _ => match Self::data_block(&inner, logical_block) {
                    Ok(block) => {
                        mapped_block = Some((logical_block, block));
                        block
                    }
                    Err(err) => {
                        return if total_written > 0 {
                            Ok(total_written)
                        } else {
                            Err(err)
                        };
                    }
                },
            };

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
        inode_bitmap_lock: SpinLock::new(()),
        data_bitmap_lock: SpinLock::new(()),
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
    fn create_file(&self, parent: &Arc<INode<BD>>, name: &[u8]) -> VfsResult<Arc<INode<BD>>> {
        if name.is_empty() || name.len() > 27 {
            return Err(VfsError::Fs);
        }

        let mut parent_inner = parent.inner.write_lock();
        if parent_inner.ty != Type::Directory
            || parent_inner.metadata.sz as usize % size_of::<DirEnt>() != 0
        {
            return Err(VfsError::Fs);
        }
        match INode::<BD>::lookup_path_inner(&parent.fs, &parent_inner, name) {
            Ok(_) => return Err(VfsError::Fs),
            Err(VfsError::NotFound) => {}
            Err(err) => return Err(err),
        }

        let original_parent = *parent_inner;
        let mut updated_parent = original_parent;
        updated_parent.metadata.sz = updated_parent
            .metadata
            .sz
            .checked_add(size_of::<DirEnt>() as u32)
            .ok_or(VfsError::Fs)?;

        let dirent_offset = original_parent.metadata.sz as usize;
        let block_idx = dirent_offset / BLOCK_SIZE;
        let block_offset = dirent_offset % BLOCK_SIZE;
        if block_idx >= updated_parent.direct_blocks.len() {
            return Err(VfsError::Fs);
        }

        let allocated_data_block = if updated_parent.direct_blocks[block_idx] == 0 {
            let block = self.allocate_data_block()?;
            updated_parent.direct_blocks[block_idx] = block;
            Some(block)
        } else {
            None
        };

        let inum = match self.allocate_inode() {
            Ok(inum) => inum,
            Err(err) => {
                self.free_create_allocations(None, allocated_data_block);
                return Err(err);
            }
        };
        let inode_inner = INodeInner {
            ty: Type::File,
            link_count: 1,
            metadata: Metadata { dev: 0, sz: 0 },
            direct_blocks: [0; 12],
            indirect_block: 0,
        };

        if let Err(err) = Self::write_inode_to_block(
            self.superblock.inode_table_start as usize,
            inum,
            &inode_inner,
        ) {
            self.free_create_allocations(Some(inum), allocated_data_block);
            return Err(err);
        }

        let mut dirent = DirEnt {
            inum: inum as u32,
            name_len: name.len() as u8,
            name: [0; 27],
        };
        dirent.name[..name.len()].copy_from_slice(name);
        if let Err(err) = Self::write_dirent(
            updated_parent.direct_blocks[block_idx],
            block_offset,
            &dirent,
        ) {
            self.free_create_allocations(Some(inum), allocated_data_block);
            return Err(err);
        }

        if let Err(err) = Self::write_inode_to_block(
            self.superblock.inode_table_start as usize,
            parent.inum,
            &updated_parent,
        ) {
            let empty_dirent = DirEnt {
                inum: 0,
                name_len: 0,
                name: [0; 27],
            };
            let _ = Self::write_dirent(
                updated_parent.direct_blocks[block_idx],
                block_offset,
                &empty_dirent,
            );
            // If this fails, the on-disk parent may still reference the new
            // resources, so leaking them is safer than freeing them.
            if Self::write_inode_to_block(
                self.superblock.inode_table_start as usize,
                parent.inum,
                &original_parent,
            )
            .is_ok()
            {
                self.free_create_allocations(Some(inum), allocated_data_block);
            }
            return Err(err);
        }

        *parent_inner = updated_parent;

        let inode = Arc::new(INode {
            inum,
            fs: parent.fs.clone(),
            inner: Arc::new(RwLock::new(inode_inner)),
            _marker: PhantomData,
        });
        self.inode_cache.lock().insert(inum, inode.clone());
        Ok(inode)
    }

    /// Best-effort cleanup for resources reserved while creating a file.
    fn free_create_allocations(&self, inum: Option<usize>, data_block: Option<u32>) {
        if let Some(inum) = inum {
            let _ = self.free_inode(inum);
        }
        if let Some(block) = data_block {
            let _ = self.free_data_block(block);
        }
    }

    fn allocate_inode(&self) -> VfsResult<usize> {
        let _guard = self.inode_bitmap_lock.lock();
        self.allocate_bitmap_bit(
            self.superblock.inode_bitmap_block,
            1,
            self.superblock.ninodes as usize,
        )
    }

    fn allocate_data_block(&self) -> VfsResult<u32> {
        let _guard = self.data_bitmap_lock.lock();
        let bit = self.allocate_bitmap_bit(
            self.superblock.data_bitmap_block,
            0,
            (self.superblock.nblocks - self.superblock.data_block_start) as usize,
        )?;

        Ok(self.superblock.data_block_start + bit as u32)
    }

    fn free_inode(&self, inum: usize) -> VfsResult<()> {
        let _guard = self.inode_bitmap_lock.lock();
        self.free_bitmap_bit(self.superblock.inode_bitmap_block, inum)
    }

    fn free_data_block(&self, block: u32) -> VfsResult<()> {
        let bit = block
            .checked_sub(self.superblock.data_block_start)
            .ok_or(VfsError::Fs)? as usize;
        let _guard = self.data_bitmap_lock.lock();
        self.free_bitmap_bit(self.superblock.data_bitmap_block, bit)
    }

    fn allocate_bitmap_bit(
        &self,
        bitmap_block: u32,
        start_bit: usize,
        end_bit: usize,
    ) -> VfsResult<usize> {
        let bits_per_sector = SECTOR_SIZE * 8;
        let start_sector = start_bit / bits_per_sector;
        let end_sector = end_bit.div_ceil(bits_per_sector);
        if end_sector > SECTORS_PER_BLOCK {
            return Err(VfsError::Fs);
        }

        let mut buf = [0; SECTOR_SIZE];
        for sector_offset in start_sector..end_sector {
            BD::read_sector(
                bitmap_block as usize * SECTORS_PER_BLOCK + sector_offset,
                &mut buf,
            )?;

            let sector_start_bit = sector_offset * bits_per_sector;
            let local_start = start_bit.saturating_sub(sector_start_bit);
            let local_end = core::cmp::min(end_bit - sector_start_bit, bits_per_sector);
            for local_bit in local_start..local_end {
                let byte_idx = local_bit / 8;
                let mask = 1 << (local_bit % 8);
                if buf[byte_idx] & mask == 0 {
                    buf[byte_idx] |= mask;
                    BD::write_sector(
                        bitmap_block as usize * SECTORS_PER_BLOCK + sector_offset,
                        &buf,
                    )?;
                    return Ok(sector_start_bit + local_bit);
                }
            }
        }

        Err(VfsError::Fs)
    }

    fn free_bitmap_bit(&self, bitmap_block: u32, bit: usize) -> VfsResult<()> {
        let sector_offset = bit / (SECTOR_SIZE * 8);
        if sector_offset >= SECTORS_PER_BLOCK {
            return Err(VfsError::Fs);
        }

        let byte_idx = bit % (SECTOR_SIZE * 8) / 8;
        let mask = 1 << (bit % 8);
        let sector = bitmap_block as usize * SECTORS_PER_BLOCK + sector_offset;
        let mut buf = [0; SECTOR_SIZE];
        BD::read_sector(sector, &mut buf)?;
        if buf[byte_idx] & mask == 0 {
            return Err(VfsError::Fs);
        }

        buf[byte_idx] &= !mask;
        BD::write_sector(sector, &buf)
    }

    fn write_inode_to_block(
        inode_table_start: usize,
        inum: usize,
        inner: &INodeInner,
    ) -> VfsResult<()> {
        let inode_byte_offset = inode_table_start * BLOCK_SIZE + inum * size_of::<INodeInner>();
        let inode_sector = inode_byte_offset / SECTOR_SIZE;
        let inode_offset = inode_byte_offset % SECTOR_SIZE;

        let bytes = unsafe {
            slice::from_raw_parts(
                inner as *const INodeInner as *const u8,
                size_of::<INodeInner>(),
            )
        };
        if inode_offset + bytes.len() > SECTOR_SIZE {
            return Err(VfsError::Fs);
        }

        let buf = &mut [0; SECTOR_SIZE];
        BD::read_sector(inode_sector, buf)?;
        buf[inode_offset..inode_offset + bytes.len()].copy_from_slice(bytes);
        BD::write_sector(inode_sector, buf)
    }

    fn write_dirent(block: u32, block_offset: usize, dirent: &DirEnt) -> VfsResult<()> {
        let sector_idx = block_offset / SECTOR_SIZE;
        let sector_offset = block_offset % SECTOR_SIZE;
        let bytes = unsafe {
            slice::from_raw_parts(dirent as *const DirEnt as *const u8, size_of::<DirEnt>())
        };
        if sector_idx >= SECTORS_PER_BLOCK || sector_offset + bytes.len() > SECTOR_SIZE {
            return Err(VfsError::Fs);
        }

        let buf = &mut [0; SECTOR_SIZE];
        let sector = block as usize * SECTORS_PER_BLOCK + sector_idx;
        BD::read_sector(sector, buf)?;
        buf[sector_offset..sector_offset + bytes.len()].copy_from_slice(bytes);
        BD::write_sector(sector, buf)
    }

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

#[cfg(test)]
mod tests {
    extern crate std;

    use super::*;
    use std::{sync::Mutex, vec, vec::Vec};

    static DISK: Mutex<Vec<u8>> = Mutex::new(Vec::new());

    struct MemoryBlockDevice;

    impl BlockDevice for MemoryBlockDevice {
        fn read_sector(sector: usize, buf: &mut [u8; SECTOR_SIZE]) -> VfsResult<()> {
            let disk = DISK.lock().unwrap();
            let start = sector.checked_mul(SECTOR_SIZE).ok_or(VfsError::DeviceIO)?;
            let end = start.checked_add(SECTOR_SIZE).ok_or(VfsError::DeviceIO)?;
            let source = disk.get(start..end).ok_or(VfsError::DeviceIO)?;
            buf.copy_from_slice(source);
            Ok(())
        }

        fn write_sector(sector: usize, buf: &[u8; SECTOR_SIZE]) -> VfsResult<()> {
            let mut disk = DISK.lock().unwrap();
            let start = sector.checked_mul(SECTOR_SIZE).ok_or(VfsError::DeviceIO)?;
            let end = start.checked_add(SECTOR_SIZE).ok_or(VfsError::DeviceIO)?;
            let destination = disk.get_mut(start..end).ok_or(VfsError::DeviceIO)?;
            destination.copy_from_slice(buf);
            Ok(())
        }
    }

    #[test]
    fn reads_and_writes_across_direct_indirect_boundary() {
        const DIRECT_DATA_BLOCK: u32 = 1;
        const INDIRECT_BLOCK: u32 = 2;
        const INDIRECT_DATA_BLOCK: u32 = 3;

        let mut disk = vec![0; 4 * BLOCK_SIZE];
        let direct_tail = DIRECT_DATA_BLOCK as usize * BLOCK_SIZE + BLOCK_SIZE - 4;
        disk[direct_tail..direct_tail + 4].copy_from_slice(b"abcd");
        let indirect_start = INDIRECT_BLOCK as usize * BLOCK_SIZE;
        disk[indirect_start..indirect_start + size_of::<u32>()]
            .copy_from_slice(&INDIRECT_DATA_BLOCK.to_ne_bytes());
        let indirect_data_start = INDIRECT_DATA_BLOCK as usize * BLOCK_SIZE;
        disk[indirect_data_start..indirect_data_start + 4].copy_from_slice(b"efgh");
        *DISK.lock().unwrap() = disk;

        let fs: Arc<Vsfs<MemoryBlockDevice>> = Arc::new(Vsfs {
            superblock: SuperBlock {
                magic: MAGIC,
                nblocks: 4,
                ninodes: 1,
                inode_bitmap_block: 0,
                data_bitmap_block: 0,
                inode_table_start: 0,
                inode_table_blocks: 0,
                data_block_start: 1,
            },
            inode_cache: SpinLock::new(BTreeMap::new()),
            inode_bitmap_lock: SpinLock::new(()),
            data_bitmap_lock: SpinLock::new(()),
            _marker: PhantomData,
        });
        let mut direct_blocks = [0; DIRECT_BLOCKS];
        direct_blocks[DIRECT_BLOCKS - 1] = DIRECT_DATA_BLOCK;
        let inode = INode {
            inum: 1,
            fs,
            inner: Arc::new(RwLock::new(INodeInner {
                ty: Type::File,
                link_count: 1,
                metadata: Metadata {
                    dev: 0,
                    sz: ((DIRECT_BLOCKS + 1) * BLOCK_SIZE) as u32,
                },
                direct_blocks,
                indirect_block: INDIRECT_BLOCK,
            })),
            _marker: PhantomData,
        };

        let boundary_offset = DIRECT_BLOCKS * BLOCK_SIZE - 4;
        let mut read_buf = [0; 8];
        assert_eq!(inode.read(boundary_offset, &mut read_buf), Ok(8));
        assert_eq!(&read_buf, b"abcdefgh");

        assert_eq!(inode.write(boundary_offset, b"ABCDEFGH"), Ok(8));
        let disk = DISK.lock().unwrap();
        assert_eq!(&disk[direct_tail..direct_tail + 4], b"ABCD");
        assert_eq!(&disk[indirect_data_start..indirect_data_start + 4], b"EFGH");
    }
}
