use alloc::{
    collections::btree_map::{BTreeMap, Entry},
    sync::Arc,
    vec::Vec,
};
use ksync::SpinLock;
use vfs::{BlockDevice, File, Filesystem, VfsError, VfsResult};

static FILE_SYSTEMS: FileSystems = FileSystems {
    file_systems: SpinLock::new(BTreeMap::new()),
};

struct FileSystems {
    file_systems: SpinLock<BTreeMap<Vec<u8>, Arc<SpinLock<dyn Filesystem>>>>,
}

pub enum SupportedFs {
    Vsfs,
}

pub fn mount<BD: BlockDevice + 'static + Send + Sync>(
    path: &[u8],
    fs_type: SupportedFs,
) -> VfsResult<()> {
    match fs_type {
        SupportedFs::Vsfs => {
            let fs = vsfs::initialize::<BD>()?;
            match FILE_SYSTEMS.file_systems.lock().entry(path.to_vec()) {
                Entry::Occupied(_) => return Err(VfsError::AlreadyMounted),
                Entry::Vacant(e) => {
                    e.insert(fs);
                }
            }
        }
    }

    Ok(())
}

pub fn open(path: &[u8]) -> VfsResult<File> {
    let mounts = FILE_SYSTEMS.file_systems.lock();
    let (mount_path, fs) = find_mount(&mounts, path).ok_or(VfsError::Fs)?;

    let relative_path = if mount_path == b"/" {
        path.strip_prefix(b"/").unwrap_or(path)
    } else {
        path.strip_prefix(mount_path)
            .unwrap_or(path)
            .strip_prefix(b"/")
            .unwrap_or(b"")
    }
    .to_vec();

    drop(mounts);

    let root = fs.lock().root()?;
    root.open(&relative_path)
}

// TODO(aeryz): This is messed up, it's a huge burden to be needing to iterate
// through all the keys. We need to store a mount tree s.t. we can easily go the
// longest matching path.
//
// Basically this:
//            "/"
//           /   \
//       "/mnt" "/mnt2"
//      /     \        \
//  "/mnt/a" "/mnt/b" "/mnt2/b"
fn find_mount<'a>(
    mounts: &'a BTreeMap<Vec<u8>, Arc<SpinLock<dyn Filesystem>>>,
    path: &[u8],
) -> Option<(&'a [u8], Arc<SpinLock<dyn Filesystem>>)> {
    mounts
        .iter()
        .filter(|(mount_path, _)| is_mount_prefix(mount_path, path))
        .max_by_key(|(mount_path, _)| mount_path.len())
        .map(|(mount_path, fs)| (mount_path.as_slice(), fs.clone()))
}

fn is_mount_prefix(mount: &[u8], path: &[u8]) -> bool {
    if mount == b"/" {
        return path.starts_with(b"/");
    }

    path == mount || path.starts_with(mount) && path.get(mount.len()) == Some(&b'/')
}

// /// Open a file
// ///
// /// TODO(aeryz): Right now, this only allows opening a file, no directory
// read /// support yet.
// /// TODO(aeryz): This only supports absolute paths right now
// pub fn open(path: &[u8]) -> Option<File> {
//     let mut current_inode = get_inode(1);
//     for path in path.split(|b| *b == b'/').filter(|p| !p.is_empty()) {
//         if current_inode.ty == Type::File {
//             return None;
//         }
//         current_inode = lookup_path(&current_inode, path).unwrap();
//     }

//     Some(File {
//         inode: Arc::new(current_inode),
//         perm: FileFlag::all(),
//         offset: 0,
//     })
// }

// pub fn read(file: &mut File, buf: &mut [u8]) -> Result<usize, ()> {
//     const BLOCK_SIZE: usize = 4096;
//     const SECTOR_SIZE: usize = 512;
//     const SECTORS_PER_BLOCK: usize = BLOCK_SIZE / SECTOR_SIZE;

//     if file.inode.ty != Type::File {
//         return Err(());
//     }

//     let file_size = file.inode.metadata.sz as usize;
//     if file.offset >= file_size || buf.is_empty() {
//         return Ok(0);
//     }

//     let mut total_read = 0;
//     let mut sector_buf = [0; SECTOR_SIZE];
//     let mut remaining = core::cmp::min(buf.len(), file_size - file.offset);

//     while remaining > 0 {
//         let logical_block = file.offset / BLOCK_SIZE;
//         if logical_block >= file.inode.direct_blocks.len() {
//             return if total_read > 0 {
//                 Ok(total_read)
//             } else {
//                 Err(())
//             };
//         }

//         let block = file.inode.direct_blocks[logical_block];
//         if block == 0 {
//             return if total_read > 0 {
//                 Ok(total_read)
//             } else {
//                 Err(())
//             };
//         }

//         let block_offset = file.offset % BLOCK_SIZE;
//         let sector_in_block = block_offset / SECTOR_SIZE;
//         let sector_offset = block_offset % SECTOR_SIZE;
//         let sector = block as usize * SECTORS_PER_BLOCK + sector_in_block;

//         if unsafe { virtio::block::read(&mut sector_buf, sector as u64) }
//             != block::VirtioBlkStatus::Ok as u8
//         {
//             return Err(());
//         }

//         let readable_from_sector = SECTOR_SIZE - sector_offset;
//         let to_copy = core::cmp::min(readable_from_sector, remaining);
//         buf[total_read..total_read + to_copy]
//             .copy_from_slice(&sector_buf[sector_offset..sector_offset +
// to_copy]);

//         file.offset += to_copy;
//         total_read += to_copy;
//         remaining -= to_copy;
//     }

//     Ok(total_read)
// }

// pub fn write(file: &mut File, buf: &[u8]) -> Result<usize, ()> {
//     const BLOCK_SIZE: usize = 4096;
//     const SECTOR_SIZE: usize = 512;
//     const SECTORS_PER_BLOCK: usize = BLOCK_SIZE / SECTOR_SIZE;

//     if file.inode.ty != Type::File {
//         return Err(());
//     }

//     let file_size = file.inode.metadata.sz as usize;
//     if file.offset >= file_size || buf.is_empty() {
//         return Ok(0);
//     }

//     let mut total_written = 0;
//     let mut sector_buf = [0; SECTOR_SIZE];
//     let mut remaining = core::cmp::min(buf.len(), file_size - file.offset);

//     while remaining > 0 {
//         let logical_block = file.offset / BLOCK_SIZE;
//         if logical_block >= file.inode.direct_blocks.len() {
//             return if total_written > 0 {
//                 Ok(total_written)
//             } else {
//                 Err(())
//             };
//         }

//         let block = file.inode.direct_blocks[logical_block];
//         if block == 0 {
//             return if total_written > 0 {
//                 Ok(total_written)
//             } else {
//                 Err(())
//             };
//         }

//         let block_offset = file.offset % BLOCK_SIZE;
//         let sector_in_block = block_offset / SECTOR_SIZE;
//         let sector_offset = block_offset % SECTOR_SIZE;
//         let sector = block as usize * SECTORS_PER_BLOCK + sector_in_block;

//         let writable_to_sector = SECTOR_SIZE - sector_offset;
//         let to_copy = core::cmp::min(writable_to_sector, remaining);

//         if to_copy != SECTOR_SIZE {
//             if unsafe { virtio::block::read(&mut sector_buf, sector as u64) }
//                 != block::VirtioBlkStatus::Ok as u8
//             {
//                 return Err(());
//             }
//         }

//         sector_buf[sector_offset..sector_offset + to_copy]
//             .copy_from_slice(&buf[total_written..total_written + to_copy]);

//         if unsafe { virtio::block::write(&sector_buf, sector as u64) }
//             != block::VirtioBlkStatus::Ok as u8
//         {
//             return Err(());
//         }

//         file.offset += to_copy;
//         total_written += to_copy;
//         remaining -= to_copy;
//     }

//     Ok(total_written)
// }

// pub enum SeekFrom {
//     Start(usize),
//     Current(isize),
//     End(isize),
// }

// pub fn seek(file: &mut File, pos: SeekFrom) -> Result<usize, ()> {
//     if file.inode.ty != Type::File {
//         return Err(());
//     }

//     let file_size = file.inode.metadata.sz as usize;
//     let new_offset = match pos {
//         SeekFrom::Start(offset) => Some(offset),
//         SeekFrom::Current(offset) => checked_offset(file.offset, offset),
//         SeekFrom::End(offset) => checked_offset(file_size, offset),
//     }
//     .ok_or(())?;

//     if new_offset > file_size {
//         return Err(());
//     }

//     file.offset = new_offset;
//     Ok(file.offset)
// }

// fn checked_offset(base: usize, offset: isize) -> Option<usize> {
//     if offset >= 0 {
//         base.checked_add(offset as usize)
//     } else {
//         base.checked_sub(offset.unsigned_abs())
//     }
// }

// fn get_inode(inum: usize) -> INode {
//     let sb = SUPERBLOCK.0.get().unwrap();

//     let inode_byte_offset = sb.inode_table_start as usize * 4096 + inum *
// size_of::<INode>();     let inode_sector = inode_byte_offset / 512;
//     let inode_offset = inode_byte_offset % 512;

//     let mut buf = &mut [0; 512];
//     if unsafe { virtio::block::read(&mut buf, inode_sector as u64) }
//         != block::VirtioBlkStatus::Ok as u8
//     {
//         panic!("block read failed");
//     }

//     unsafe { (*(buf[inode_offset..].as_ptr() as *const _ as *const
// INode)).clone() } }
