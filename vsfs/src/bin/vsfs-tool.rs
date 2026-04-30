use std::fs::File;
use std::io::{self, Seek, SeekFrom, Write};

use vsfs::{INode, Metadata, SuperBlock, Type};

const BLOCK_SIZE: usize = 4096;
const NBLOCKS: u32 = 64;
const NINODES: u32 = 80;

const SUPER_BLOCK: u32 = 0;
const INODE_BITMAP_BLOCK: u32 = 1;
const DATA_BITMAP_BLOCK: u32 = 2;
const INODE_TABLE_START: u32 = 3;
const INODE_TABLE_BLOCKS: u32 = 5;
const DATA_BLOCK_START: u32 = 8;

const ROOT_INO: u32 = 1;
const HELLO_INO: u32 = 2;

const ROOT_DATA_BLOCK: u32 = 8;
const HELLO_DATA_BLOCK: u32 = 9;

const VSFS_MAGIC: u32 = 0x5653_4653; // "VSFS"

#[repr(C)]
#[derive(Clone, Copy)]
struct DirEnt {
    inum: u32,
    name: [u8; 28],
}

fn main() -> io::Result<()> {
    let mut img = File::create("vsfs.img")?;

    img.set_len((NBLOCKS as usize * BLOCK_SIZE) as u64)?;

    write_superblock(&mut img)?;
    write_inode_bitmap(&mut img)?;
    write_data_bitmap(&mut img)?;
    write_root_inode(&mut img)?;
    write_root_dir_block(&mut img)?;
    write_hello_file(&mut img)?;

    Ok(())
}

fn block_offset(block: u32) -> u64 {
    block as u64 * BLOCK_SIZE as u64
}

fn write_at_block(img: &mut File, block: u32, buf: &[u8]) -> io::Result<()> {
    assert!(buf.len() <= BLOCK_SIZE);

    img.seek(SeekFrom::Start(block_offset(block)))?;
    img.write_all(buf)?;

    Ok(())
}

fn write_struct<T>(img: &mut File, offset: u64, val: &T) -> io::Result<()> {
    let bytes = unsafe {
        std::slice::from_raw_parts(val as *const T as *const u8, std::mem::size_of::<T>())
    };

    img.seek(SeekFrom::Start(offset))?;
    img.write_all(bytes)?;

    Ok(())
}

fn inode_offset(inum: u32) -> u64 {
    let inode_size = std::mem::size_of::<INode>() as u64;

    block_offset(INODE_TABLE_START) + inum as u64 * inode_size
}

fn write_superblock(img: &mut File) -> io::Result<()> {
    let sb = SuperBlock {
        magic: VSFS_MAGIC,
        nblocks: NBLOCKS,
        ninodes: NINODES,
        inode_bitmap_block: INODE_BITMAP_BLOCK,
        data_bitmap_block: DATA_BITMAP_BLOCK,
        inode_table_start: INODE_TABLE_START,
        inode_table_blocks: INODE_TABLE_BLOCKS,
        data_block_start: DATA_BLOCK_START,
    };

    write_struct(img, block_offset(SUPER_BLOCK), &sb)
}

fn set_bit(bitmap: &mut [u8], bit: u32) {
    let byte = bit / 8;
    let off = bit % 8;
    bitmap[byte as usize] |= 1 << off;
}

fn write_inode_bitmap(img: &mut File) -> io::Result<()> {
    let mut bitmap = [0u8; BLOCK_SIZE];

    set_bit(&mut bitmap, ROOT_INO);
    set_bit(&mut bitmap, HELLO_INO);

    write_at_block(img, INODE_BITMAP_BLOCK, &bitmap)
}

fn write_data_bitmap(img: &mut File) -> io::Result<()> {
    let mut bitmap = [0u8; BLOCK_SIZE];

    // Data bitmap is usually relative to DATA_BLOCK_START.
    set_bit(&mut bitmap, ROOT_DATA_BLOCK - DATA_BLOCK_START);
    set_bit(&mut bitmap, HELLO_DATA_BLOCK - DATA_BLOCK_START);

    write_at_block(img, DATA_BITMAP_BLOCK, &bitmap)
}

fn write_root_inode(img: &mut File) -> io::Result<()> {
    let mut direct_blocks = [0u32; 12];
    direct_blocks[0] = ROOT_DATA_BLOCK;

    let inode = INode {
        ty: Type::Directory,
        link_count: 1,
        metadata: Metadata {
            sz: (3 * std::mem::size_of::<DirEnt>()) as u32,
            dev: 0,
        },
        direct_blocks,
        indirect_block: 0,
    };

    write_struct(img, inode_offset(ROOT_INO), &inode)
}

fn write_root_dir_block(img: &mut File) -> io::Result<()> {
    let entries = [
        dirent(ROOT_INO, "."),
        dirent(ROOT_INO, ".."),
        dirent(HELLO_INO, "hello"),
    ];

    let bytes = unsafe {
        std::slice::from_raw_parts(
            entries.as_ptr() as *const u8,
            std::mem::size_of_val(&entries),
        )
    };

    write_at_block(img, ROOT_DATA_BLOCK, bytes)
}

fn write_hello_file(img: &mut File) -> io::Result<()> {
    let data = b"hello world\n";

    let mut direct_blocks = [0u32; 12];
    direct_blocks[0] = HELLO_DATA_BLOCK;

    let inode = INode {
        ty: Type::File,
        link_count: 1,
        metadata: Metadata {
            dev: 0,
            sz: data.len() as u32,
        },
        direct_blocks,
        indirect_block: 0,
    };

    write_struct(img, inode_offset(HELLO_INO), &inode)?;
    write_at_block(img, HELLO_DATA_BLOCK, data)?;

    Ok(())
}

fn dirent(inum: u32, name: &str) -> DirEnt {
    let mut out = DirEnt {
        inum,
        name: [0u8; 28],
    };

    let bytes = name.as_bytes();
    assert!(bytes.len() <= out.name.len());

    out.name[..bytes.len()].copy_from_slice(bytes);

    out
}
