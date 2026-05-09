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

pub struct Vsfs<BD: BlockDevice> {
    superblock: SuperBlock,
    inode_cache: BTreeMap<usize, Arc<INode<BD>>>,
    _marker: PhantomData<BD>,
}

pub enum Error {}

#[derive(Derivative)]
#[derivative(Clone(bound = ""))]
pub struct INode<BD: BlockDevice> {
    inum: usize,
    fs: Arc<SpinLock<Vsfs<BD>>>,
    inner: Arc<RwLock<INodeInner>>,
    _marker: PhantomData<BD>,
}

#[derive(Clone, Copy)]
#[repr(C)]
pub struct INodeInner {
    pub ty: Type,
    pub link_count: u16,
    pub metadata: Metadata,
    pub direct_blocks: [u32; 12],
    pub indirect_block: u32,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[repr(u16)]
pub enum Type {
    Directory = 1,
    File = 2,
}

#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct Metadata {
    /// ID of the device containing file
    // TODO(aeryz): should I have a dev_t?
    pub dev: u32,
    /// Total size of the file in bytes
    pub sz: u32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct DirEnt {
    pub inum: u32,
    pub name_len: u8,
    pub name: [u8; 27],
}

impl<BD: BlockDevice> INode<BD> {
    fn lookup_path(
        fs: &Arc<SpinLock<Vsfs<BD>>>,
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
                            buf[(size_of::<DirEnt>() * cur_dir_idx)..].as_ptr() as *const DirEnt,
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
}

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

pub fn initialize<BD: BlockDevice>() -> VfsResult<Arc<SpinLock<Vsfs<BD>>>> {
    let buf = &mut [0; 512];
    BD::read_sector(0, buf)?;

    let sb = unsafe { ptr::read_unaligned(buf.as_ptr() as *const SuperBlock) };

    if sb.magic != MAGIC {
        return Err(VfsError::Fs);
    }

    let vsfs = Arc::new(SpinLock::new(Vsfs {
        superblock: sb,
        inode_cache: BTreeMap::new(),
        _marker: PhantomData,
    }));

    let root_inode = Vsfs::<BD>::read_inode(vsfs.clone(), 1)?;
    let _ = vsfs.lock().inode_cache.insert(1, root_inode);

    Ok(vsfs)
}
impl<BD: BlockDevice + 'static + Send + Sync> Filesystem for Vsfs<BD> {
    fn root(&self) -> VfsResult<Arc<dyn VNode>> {
        Ok(self
            .inode_cache
            .get(&1)
            .expect("root inode always exists")
            .clone())
    }
}

impl<BD: BlockDevice> Vsfs<BD> {
    fn read_inode(fs: Arc<SpinLock<Self>>, inum: usize) -> VfsResult<Arc<INode<BD>>> {
        let mut fs_ = fs.lock();
        let inode_table_start = fs_.superblock.inode_table_start;
        match fs_.inode_cache.entry(inum) {
            Entry::Vacant(inode) => {
                let i = Arc::new(INode {
                    inum,
                    fs: fs.clone(),
                    inner: Arc::new(RwLock::new(Self::read_inode_from_block(
                        inode_table_start as usize,
                        inum,
                    )?)),
                    _marker: PhantomData,
                });
                inode.insert_entry(i.clone());
                Ok(i)
            }
            Entry::Occupied(occupied_entry) => Ok(occupied_entry.get().clone()),
        }
    }

    fn read_inode_from_block(inode_table_start: usize, inum: usize) -> VfsResult<INodeInner> {
        let inode_byte_offset = inode_table_start * 4096 + inum * size_of::<INodeInner>();
        let inode_sector = inode_byte_offset / 512;
        let inode_offset = inode_byte_offset % 512;

        let buf = &mut [0; 512];

        BD::read_sector(inode_sector, buf)?;

        let inner = unsafe {
            ptr::read_unaligned(buf[inode_offset..].as_ptr() as *const INodeInner)
        };

        Ok(inner)
    }
}
