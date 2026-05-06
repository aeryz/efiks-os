pub mod directory;
mod file;
mod inode;

use core::{cell::OnceCell, mem::MaybeUninit};

use alloc::sync::Arc;
pub use file::*;
pub use inode::*;

use crate::driver::virtio::{self, block};
use vsfs::{DirEnt, INode, SuperBlock, Type};

const MAX_DIRENTS_IN_SECTOR: usize = 512 / size_of::<DirEnt>();

static SUPERBLOCK: SuperBlockSend = SuperBlockSend(OnceCell::new());

struct SuperBlockSend(OnceCell<SuperBlock>);

// This will only be initialized once by a single core/thread.
unsafe impl Send for SuperBlockSend {}
unsafe impl Sync for SuperBlockSend {}

pub fn init() {
    SUPERBLOCK.0.get_or_init(|| {
        let mut buf = &mut [0; 512];
        if unsafe { virtio::block::read(&mut buf, 0) } != block::VirtioBlkStatus::Ok as u8 {
            panic!("block read failed");
        }

        let sb = unsafe {
            (buf.as_ptr() as *const _ as *const SuperBlock)
                .as_ref()
                .unwrap()
        };

        log::trace!("Superblock: {:?}", sb);
        *sb
    });
}

/// Open a file
///
/// TODO(aeryz): Right now, this only allows opening a file, no directory read
/// support yet.
/// TODO(aeryz): This only supports absolute paths right now
pub fn open(path: &[u8]) -> Option<File> {
    let mut current_inode = get_inode(1);
    for path in path.split(|b| *b == b'/').filter(|p| !p.is_empty()) {
        if current_inode.ty == Type::File {
            return None;
        }
        current_inode = lookup_path(&current_inode, path).unwrap();
    }

    Some(File {
        inode: Arc::new(current_inode),
        perm: FileFlag::all(),
        offset: 0,
    })
}

pub fn read(file: &mut File, buf: &mut [u8]) -> Result<usize, ()> {
    const BLOCK_SIZE: usize = 4096;
    const SECTOR_SIZE: usize = 512;
    const SECTORS_PER_BLOCK: usize = BLOCK_SIZE / SECTOR_SIZE;

    if file.inode.ty != Type::File {
        return Err(());
    }

    let file_size = file.inode.metadata.sz as usize;
    if file.offset >= file_size || buf.is_empty() {
        return Ok(0);
    }

    let mut total_read = 0;
    let mut sector_buf = [0; SECTOR_SIZE];
    let mut remaining = core::cmp::min(buf.len(), file_size - file.offset);

    while remaining > 0 {
        let logical_block = file.offset / BLOCK_SIZE;
        if logical_block >= file.inode.direct_blocks.len() {
            return if total_read > 0 {
                Ok(total_read)
            } else {
                Err(())
            };
        }

        let block = file.inode.direct_blocks[logical_block];
        if block == 0 {
            return if total_read > 0 {
                Ok(total_read)
            } else {
                Err(())
            };
        }

        let block_offset = file.offset % BLOCK_SIZE;
        let sector_in_block = block_offset / SECTOR_SIZE;
        let sector_offset = block_offset % SECTOR_SIZE;
        let sector = block as usize * SECTORS_PER_BLOCK + sector_in_block;

        if unsafe { virtio::block::read(&mut sector_buf, sector as u64) }
            != block::VirtioBlkStatus::Ok as u8
        {
            return Err(());
        }

        let readable_from_sector = SECTOR_SIZE - sector_offset;
        let to_copy = core::cmp::min(readable_from_sector, remaining);
        buf[total_read..total_read + to_copy]
            .copy_from_slice(&sector_buf[sector_offset..sector_offset + to_copy]);

        file.offset += to_copy;
        total_read += to_copy;
        remaining -= to_copy;
    }

    Ok(total_read)
}

pub fn write(file: &mut File, buf: &[u8]) -> Result<usize, ()> {
    const BLOCK_SIZE: usize = 4096;
    const SECTOR_SIZE: usize = 512;
    const SECTORS_PER_BLOCK: usize = BLOCK_SIZE / SECTOR_SIZE;

    if file.inode.ty != Type::File {
        return Err(());
    }

    let file_size = file.inode.metadata.sz as usize;
    if file.offset >= file_size || buf.is_empty() {
        return Ok(0);
    }

    let mut total_written = 0;
    let mut sector_buf = [0; SECTOR_SIZE];
    let mut remaining = core::cmp::min(buf.len(), file_size - file.offset);

    while remaining > 0 {
        let logical_block = file.offset / BLOCK_SIZE;
        if logical_block >= file.inode.direct_blocks.len() {
            return if total_written > 0 {
                Ok(total_written)
            } else {
                Err(())
            };
        }

        let block = file.inode.direct_blocks[logical_block];
        if block == 0 {
            return if total_written > 0 {
                Ok(total_written)
            } else {
                Err(())
            };
        }

        let block_offset = file.offset % BLOCK_SIZE;
        let sector_in_block = block_offset / SECTOR_SIZE;
        let sector_offset = block_offset % SECTOR_SIZE;
        let sector = block as usize * SECTORS_PER_BLOCK + sector_in_block;

        let writable_to_sector = SECTOR_SIZE - sector_offset;
        let to_copy = core::cmp::min(writable_to_sector, remaining);

        if to_copy != SECTOR_SIZE {
            if unsafe { virtio::block::read(&mut sector_buf, sector as u64) }
                != block::VirtioBlkStatus::Ok as u8
            {
                return Err(());
            }
        }

        sector_buf[sector_offset..sector_offset + to_copy]
            .copy_from_slice(&buf[total_written..total_written + to_copy]);

        if unsafe { virtio::block::write(&sector_buf, sector as u64) }
            != block::VirtioBlkStatus::Ok as u8
        {
            return Err(());
        }

        file.offset += to_copy;
        total_written += to_copy;
        remaining -= to_copy;
    }

    Ok(total_written)
}

pub enum SeekFrom {
    Start(usize),
    Current(isize),
    End(isize),
}

pub fn seek(file: &mut File, pos: SeekFrom) -> Result<usize, ()> {
    if file.inode.ty != Type::File {
        return Err(());
    }

    let file_size = file.inode.metadata.sz as usize;
    let new_offset = match pos {
        SeekFrom::Start(offset) => Some(offset),
        SeekFrom::Current(offset) => checked_offset(file.offset, offset),
        SeekFrom::End(offset) => checked_offset(file_size, offset),
    }
    .ok_or(())?;

    if new_offset > file_size {
        return Err(());
    }

    file.offset = new_offset;
    Ok(file.offset)
}

fn checked_offset(base: usize, offset: isize) -> Option<usize> {
    if offset >= 0 {
        base.checked_add(offset as usize)
    } else {
        base.checked_sub(offset.unsigned_abs())
    }
}

fn get_inode(inum: usize) -> INode {
    let sb = SUPERBLOCK.0.get().unwrap();

    let inode_byte_offset = sb.inode_table_start as usize * 4096 + inum * size_of::<INode>();
    let inode_sector = inode_byte_offset / 512;
    let inode_offset = inode_byte_offset % 512;

    let mut buf = &mut [0; 512];
    if unsafe { virtio::block::read(&mut buf, inode_sector as u64) }
        != block::VirtioBlkStatus::Ok as u8
    {
        panic!("block read failed");
    }

    unsafe { (*(buf[inode_offset..].as_ptr() as *const _ as *const INode)).clone() }
}

/// Lookup the `path` inside `inode`. `inode` needs to be a directory.
fn lookup_path(inode: &INode, path: &[u8]) -> Result<INode, ()> {
    if inode.ty != Type::Directory {
        return Err(());
    }

    let n_dirent_in_node = inode.metadata.sz as usize / size_of::<DirEnt>();

    for (block_idx, block) in inode.direct_blocks.iter().enumerate() {
        let mut sector = block * 4096 / 512;

        for sector_idx in 0..8 {
            let mut buf = &mut [0; 512];
            unsafe {
                // TODO(aeryz): implement a macro or fn for this
                if virtio::block::read(&mut buf, sector as u64) != block::VirtioBlkStatus::Ok as u8
                {
                    return Err(());
                }
            }

            for cur_dir_idx in 0..MAX_DIRENTS_IN_SECTOR {
                if (block_idx * 8 + sector_idx) * MAX_DIRENTS_IN_SECTOR + cur_dir_idx
                    >= n_dirent_in_node
                {
                    return Err(());
                }

                let dirent = unsafe {
                    (buf[(size_of::<DirEnt>() * cur_dir_idx)..].as_ptr() as *const _
                        as *const DirEnt)
                        .as_ref()
                        .unwrap()
                };

                if &dirent.name[0..dirent.name_len as usize] == path {
                    return Ok(get_inode(dirent.inum as usize));
                }
            }

            sector += 1;
        }
    }

    Err(())
}
