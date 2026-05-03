pub mod directory;
mod file;
mod inode;

use core::{cell::OnceCell, mem::MaybeUninit};

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
pub fn open(path: &[u8]) -> Option<()> {
    let root_inode = get_inode(1);
    for path in path.split(|b| *b == b'/') {}

    None
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

/*

for (block_idx, block) in direct_blocks:
    sector = block * 4096 / 512;

    for sector_idx in 0..8 {
        buf = virtio::read(sector); /* contains n dirents */

        for cur_dir_idx in 0..MAX_DIRENTS_IN_SECTOR {

            if block_idx * 8 + (sector_idx * MAX_DIRENTS_IN_SECTOR) + cur_dir_idx > n_dirent_in_inode {
                // we reached the max
                break;
            }
        }


        sector += 512;
    }

*/

fn lookup_path(inode: &INode, path: &[u8]) {}

// pub fn path_traversal_example() {
//     let mut buf = &mut [0; 512];
//     let (superblock, root_inode_ptr) = {
//         if unsafe { virtio::block::read(&mut buf, 0) } !=
// block::VirtioBlkStatus::Ok as u8 {             panic!("block read failed");
//         }

//         let sb = unsafe {
//             (buf.as_ptr() as *const _ as *const SuperBlock)
//                 .as_ref()
//                 .unwrap()
//         };
//         log::info!("Superblock: {:?}", sb);

//         (
//             *sb,
//             sb.inode_table_start as usize * 4096 + ROOT_INO *
// size_of::<INode>(),         )
//     };

//     let sector = root_inode_ptr / 512;
//     let offset = root_inode_ptr % 512;

//     let (root_data_sector, n_dirents) = {
//         if unsafe { virtio::block::read(&mut buf, sector as u64) }
//             != block::VirtioBlkStatus::Ok as u8
//         {
//             panic!("block read failed");
//         }

//         log::info!("Reading from the sector: {sector} with offset:
// {offset}");

//         let inode = unsafe {
//             (buf[offset..].as_ptr() as *const _ as *const INode)
//                 .as_ref()
//                 .unwrap()
//         };

//         if inode.link_count == 0 || inode.ty != Type::Directory {
//             panic!("the root directory is a valid directory");
//         }

//         (
//             inode.direct_blocks[0] as usize * 4096 / 512,
//             inode.metadata.sz as usize / size_of::<DirEnt>(),
//         )
//     };

//     if unsafe { virtio::block::read(&mut buf, root_data_sector as u64) }
//         != block::VirtioBlkStatus::Ok as u8
//     {
//         panic!("block read failed");
//     }

//     let foo_data = (0..n_dirents)
//         .find_map(|i| {
//             let entry = unsafe {
//                 (buf.as_ptr().byte_offset((size_of::<DirEnt>() * i) as isize)
// as *const _                     as *const DirEnt)
//                     .as_ref()
//                     .unwrap()
//             };
//             log::info!("entry: {entry:?}");

//             if &entry.name[0..entry.len as usize] == b"foo" {
//                 Some(entry)
//             } else {
//                 None
//             }
//         })
//         .expect("couldn't find foo");

//     let foo_inode_ptr =
//         superblock.inode_table_start as usize * 4096 + foo_data.inum as usize
// * size_of::<INode>();     let sector = foo_inode_ptr / 512; let offset =
//   foo_inode_ptr % 512;

//     log::error!("foo inode ptr: {foo_inode_ptr} sector: {sector} offset:
// {offset}");

//     let (foo_data_sector, n_dirents) = {
//         if unsafe { virtio::block::read(&mut buf, sector as u64) }
//             != block::VirtioBlkStatus::Ok as u8
//         {
//             panic!("block read failed");
//         }

//         log::info!("Reading from the sector: {sector} with offset:
// {offset}");

//         let inode = unsafe {
//             (buf[offset..].as_ptr() as *const _ as *const INode)
//                 .as_ref()
//                 .unwrap()
//         };

//         log::error!("foo inode: {inode:?}");

//         if inode.link_count == 0 || inode.ty != Type::Directory {
//             panic!("foo directory is a valid directory");
//         }

//         (
//             inode.direct_blocks[0] as usize * 4096 / 512,
//             inode.metadata.sz as usize / size_of::<DirEnt>(),
//         )
//     };

//     if unsafe { virtio::block::read(&mut buf, foo_data_sector as u64) }
//         != block::VirtioBlkStatus::Ok as u8
//     {
//         panic!("block read failed");
//     }

//     let bar_data = (0..n_dirents)
//         .find_map(|i| {
//             let entry = unsafe {
//                 (buf.as_ptr().byte_offset((size_of::<DirEnt>() * i) as isize)
// as *const _                     as *const DirEnt)
//                     .as_ref()
//                     .unwrap()
//             };
//             log::info!("entry: {entry:?}");

//             if &entry.name[0..entry.len as usize] == b"bar" {
//                 Some(entry)
//             } else {
//                 None
//             }
//         })
//         .expect("couldn't find foo");

//     let bar_inode_ptr =
//         superblock.inode_table_start as usize * 4096 + bar_data.inum as usize
// * size_of::<INode>();     let sector = bar_inode_ptr / 512; let offset =
//   bar_inode_ptr % 512;

//     log::error!("bar inode ptr: {bar_inode_ptr} sector: {sector} offset:
// {offset}");

//     let (bar_data_sector, data_sz) = {
//         if unsafe { virtio::block::read(&mut buf, sector as u64) }
//             != block::VirtioBlkStatus::Ok as u8
//         {
//             panic!("block read failed");
//         }

//         log::info!("Reading from the sector: {sector} with offset:
// {offset}");

//         let inode = unsafe {
//             (buf[offset..].as_ptr() as *const _ as *const INode)
//                 .as_ref()
//                 .unwrap()
//         };

//         log::error!("bar inode: {inode:?}");

//         if inode.link_count == 0 || inode.ty != Type::File {
//             panic!("bar is a valid file");
//         }

//         (
//             inode.direct_blocks[0] as usize * 4096 / 512,
//             inode.metadata.sz,
//         )
//     };

//     if unsafe { virtio::block::read(&mut buf, bar_data_sector as u64) }
//         != block::VirtioBlkStatus::Ok as u8
//     {
//         panic!("block read failed");
//     }

//     unsafe {
//         panic!(
//             "data: {}",
//             str::from_utf8_unchecked(&buf[0..data_sz as usize])
//         );
//     }
// }
