#![no_std]

/*
1. Divide the disk into blocks of 4K. And we have 64 4K blocks.

[0..7] [8..15] [..] [56..63]

Regions:
1. Data region. (for user data)
2. Inode region. (for inodes)
3. Superblock. (contains the info about where the inode and data regions begins etc)

2. Blocks [3, 7] contain inodes and [8, 63] contain data. Assuming an inode is 256 bytes
in size, a block can contain 16 inodes.

3. We have 1 bitmap for inodes and 1 bitmap for the data to keep track of the allocations.
inodes bitmap go into the index 1, and the data bitmap goes into index 2.

4. Have a superblock S that contains the info about the current file system and a magic. And
let's put that to the remaining index 0.

5. In vsfs, given an inode number, you should directly be able to compute the corresponding
location in the block.

## Multi-level index

- To support bigger files, we can use indirect pointers.

## Directories

Directories just contain a list of `entry_name`, `inode number` pairs.

For a 4 item dir, it might look like:
inum  |  reclen  |  strlen  |  name
 5         12         2         .
 2         12         3         ..
 12        12         4         foo
 13        12         4         bar
 24        36        28         foobar_is_a_pretty_longname


*/

#[derive(Debug, Clone)]
#[repr(C)]
pub struct INode {
    pub ty: Type,
    pub link_count: u16,
    pub metadata: Metadata,
    pub direct_blocks: [u32; 12],
    pub indirect_block: u32,
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

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[repr(u16)]
pub enum Type {
    Directory = 1,
    File = 2,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct SuperBlock {
    pub magic: u32,
    pub nblocks: u32,
    pub ninodes: u32,
    pub inode_bitmap_block: u32,
    pub data_bitmap_block: u32,
    pub inode_table_start: u32,
    pub inode_table_blocks: u32,
    pub data_block_start: u32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct DirEnt {
    pub inum: u32,
    pub len: u8,
    pub name: [u8; 27],
}
