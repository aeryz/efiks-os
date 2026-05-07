#![no_std]

use core::marker::PhantomData;

use alloc::{
    collections::btree_map::{BTreeMap, Entry},
    sync::Arc,
};
use derivative::Derivative;
use ksync::{ReadLockGuard, RwLock, SpinLock};
use vfs::{BlockDevice, File, Filesystem, VNode, VfsError, VfsResult};

extern crate alloc;

mod lib2;

const MAGIC: u32 = 0x5653_4653; // "VSFS"
const MAX_DIRENTS_IN_SECTOR: usize = 512 / size_of::<DirEnt>();

pub struct Vsfs<BD: BlockDevice> {
    superblock: SuperBlock,
    inode_cache: BTreeMap<usize, INode<BD>>,
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

#[derive(Clone)]
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

#[derive(Debug, Clone)]
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
    ) -> VfsResult<Self> {
        if inode.ty != Type::Directory {
            return Err(vfs::VfsError::Fs);
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
                        return Err(vfs::VfsError::Fs);
                    }

                    let dirent = unsafe {
                        (buf[(size_of::<DirEnt>() * cur_dir_idx)..].as_ptr() as *const _
                            as *const DirEnt)
                            .as_ref()
                            .unwrap()
                    };

                    if &dirent.name[0..dirent.name_len as usize] == path {
                        return Ok(Vsfs::<BD>::read_inode(fs.clone(), dirent.inum as usize)?);
                    }
                }

                sector += 1;
            }
        }

        Err(vfs::VfsError::Fs)
    }
}

impl<BD: BlockDevice> VNode for INode<BD> {
    fn open(&self, path: &[u8]) -> VfsResult<File<Self>> {
        let mut current = self.clone();
        for path in path.split(|b| *b == b'/').filter(|p| !p.is_empty()) {
            let inode = current.inner.read_lock();
            if inode.ty == Type::File {
                return Err(vfs::VfsError::Fs);
            }
            let next_inode = Self::lookup_path(&self.fs, inode, path)?;

            if current.inum != next_inode.inum {
                current = next_inode;
            }
        }

        Ok(File {
            inode: Arc::new(current),
            offset: 0,
        })
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

impl<BD: BlockDevice> Filesystem for Vsfs<BD> {
    type VNode = INode<BD>;

    fn initialize() -> VfsResult<Arc<SpinLock<Self>>> {
        let buf = &mut [0; 512];
        BD::read_sector(0, buf)?;

        let sb = unsafe {
            (buf.as_ptr() as *const _ as *const SuperBlock)
                .as_ref()
                .ok_or(VfsError::Fs)?
        };

        if sb.magic != MAGIC {
            return Err(VfsError::Fs);
        }

        let vsfs = Arc::new(SpinLock::new(Self {
            superblock: *sb,
            inode_cache: BTreeMap::new(),
            _marker: PhantomData,
        }));

        let root_inode = Vsfs::<BD>::read_inode(vsfs.clone(), 1)?;
        let _ = vsfs.lock().inode_cache.insert(1, root_inode);

        Ok(vsfs)
    }

    fn root(fs: Arc<SpinLock<Self>>) -> VfsResult<INode<BD>> {
        Ok(fs
            .lock()
            .inode_cache
            .get(&1)
            .expect("root inode always exists")
            .clone())
    }
}

impl<BD: BlockDevice> Vsfs<BD> {
    fn read_inode(fs: Arc<SpinLock<Self>>, inum: usize) -> VfsResult<INode<BD>> {
        let mut fs_ = fs.lock();
        let inode_table_start = fs_.superblock.inode_table_start;
        match fs_.inode_cache.entry(inum) {
            Entry::Vacant(inode) => {
                let i = INode {
                    inum,
                    fs: fs.clone(),
                    inner: Arc::new(RwLock::new(Self::read_inode_from_block(
                        inode_table_start as usize,
                        inum,
                    )?)),
                    _marker: PhantomData,
                };
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

        let inner =
            unsafe { (*(buf[inode_offset..].as_ptr() as *const _ as *const INodeInner)).clone() };

        Ok(inner)
    }
}
