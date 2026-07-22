//! This tool is written by Codex. I'm not interested in building a tool
//! to create a fs image for me. But I'm it is surely very handy.

use std::collections::BTreeSet;
use std::env;
use std::fs::{self, File};
use std::io::{self, Seek, SeekFrom, Write};
use std::path::Path;

use serde::Deserialize;
use vsfs::{DirEnt, INodeInner, Metadata, Type};

const BLOCK_SIZE: usize = 4096;
const DIRECT_BLOCKS: usize = 12;
const INDIRECT_BLOCK_ENTRIES: usize = BLOCK_SIZE / std::mem::size_of::<u32>();
const MAX_DATA_BLOCKS: usize = DIRECT_BLOCKS + INDIRECT_BLOCK_ENTRIES;
const NBLOCKS: u32 = 64;
const NINODES: u32 = 80;

const SUPER_BLOCK: u32 = 0;
const INODE_BITMAP_BLOCK: u32 = 1;
const DATA_BITMAP_BLOCK: u32 = 2;
const INODE_TABLE_START: u32 = 3;
const INODE_TABLE_BLOCKS: u32 = 5;
const DATA_BLOCK_START: u32 = 8;

const ROOT_INO: u32 = 1;
const VSFS_MAGIC: u32 = 0x5653_4653; // "VSFS"

#[repr(C)]
#[derive(Clone, Copy)]
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

#[derive(Debug)]
struct LayoutNode {
    name: String,
    kind: LayoutKind,
}

#[derive(Debug)]
enum LayoutKind {
    Directory { children: Vec<LayoutNode> },
    File { data: Vec<u8> },
}

#[derive(Debug)]
struct FsNode {
    inum: u32,
    data_blocks: Vec<u32>,
    indirect_block: u32,
    name: String,
    kind: FsNodeKind,
}

#[derive(Debug)]
enum FsNodeKind {
    Directory { children: Vec<FsNode> },
    File { data: Vec<u8> },
}

#[derive(Debug, Deserialize)]
struct JsonLayoutNode {
    #[serde(rename = "type")]
    ty: String,
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    contains: Vec<JsonLayoutNode>,
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    contents: Option<String>,
    #[serde(default)]
    source: Option<String>,
}

fn main() -> io::Result<()> {
    let args: Vec<String> = env::args().collect();
    let config = ToolConfig::parse(&args).map_err(invalid_input)?;

    let layout = match config.input {
        Some(ToolInput::Layout(path)) => parse_layout_file(Path::new(&path))?,
        Some(ToolInput::Root(path)) => parse_root_dir(Path::new(&path))?,
        None => default_layout(),
    };

    let fs = allocate_layout(layout).map_err(invalid_input)?;
    write_image(&config.output_path, &fs)?;

    Ok(())
}

struct ToolConfig {
    output_path: String,
    input: Option<ToolInput>,
}

enum ToolInput {
    Layout(String),
    Root(String),
}

impl ToolConfig {
    fn parse(args: &[String]) -> Result<Self, String> {
        let mut output_path = String::from("vsfs.img");
        let mut input = None;
        let mut i = 1;

        while i < args.len() {
            match args[i].as_str() {
                "-o" | "--output" => {
                    i += 1;
                    output_path = args
                        .get(i)
                        .ok_or_else(|| "missing value after --output".to_string())?
                        .clone();
                }
                "-l" | "--layout" => {
                    i += 1;
                    let path = args
                        .get(i)
                        .ok_or_else(|| "missing value after --layout".to_string())?
                        .clone();
                    set_input(&mut input, ToolInput::Layout(path))?;
                }
                "-r" | "--root" => {
                    i += 1;
                    let path = args
                        .get(i)
                        .ok_or_else(|| "missing value after --root".to_string())?
                        .clone();
                    set_input(&mut input, ToolInput::Root(path))?;
                }
                "-h" | "--help" => {
                    print_usage(&args[0]);
                    std::process::exit(0);
                }
                arg if input.is_none() => {
                    set_input(&mut input, ToolInput::Layout(arg.to_string()))?;
                }
                arg => return Err(format!("unexpected argument: {arg}")),
            }

            i += 1;
        }

        Ok(Self { output_path, input })
    }
}

fn set_input(input: &mut Option<ToolInput>, new_input: ToolInput) -> Result<(), String> {
    if input.is_some() {
        return Err("--layout and --root are mutually exclusive".to_string());
    }

    *input = Some(new_input);
    Ok(())
}

fn print_usage(program: &str) {
    eprintln!("usage: {program} [--layout layout.json | --root root-dir] [--output vsfs.img]");
    eprintln!("       {program} layout.json");
}

fn default_layout() -> LayoutNode {
    LayoutNode {
        name: String::new(),
        kind: LayoutKind::Directory {
            children: vec![LayoutNode {
                name: String::from("foo"),
                kind: LayoutKind::Directory {
                    children: vec![LayoutNode {
                        name: String::from("bar"),
                        kind: LayoutKind::File {
                            data: b"hello world\n".to_vec(),
                        },
                    }],
                },
            }],
        },
    }
}

fn parse_root_dir(path: &Path) -> io::Result<LayoutNode> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.file_type().is_dir() {
        return Err(invalid_input(format!(
            "--root path `{}` is not a directory",
            path.display()
        )));
    }

    Ok(LayoutNode {
        name: String::new(),
        kind: LayoutKind::Directory {
            children: read_dir_children(path)?,
        },
    })
}

fn read_dir_children(path: &Path) -> io::Result<Vec<LayoutNode>> {
    let mut entries = Vec::new();
    for entry in fs::read_dir(path)? {
        entries.push(entry?);
    }
    entries.sort_by_key(|entry| entry.file_name());

    let mut children = Vec::with_capacity(entries.len());
    let mut names = BTreeSet::new();
    for entry in entries {
        let child_name = entry.file_name().into_string().map_err(|name| {
            invalid_input(format!(
                "path `{}` contains a non-UTF-8 filename: {:?}",
                path.display(),
                name
            ))
        })?;

        validate_name(&child_name)?;
        if !names.insert(child_name.clone()) {
            return Err(invalid_input(format!(
                "duplicate child name `{}` in directory `{}`",
                child_name,
                path.display()
            )));
        }

        children.push(layout_from_host_path(&entry.path(), child_name)?);
    }

    Ok(children)
}

fn layout_from_host_path(path: &Path, name: String) -> io::Result<LayoutNode> {
    let metadata = fs::symlink_metadata(path)?;
    let file_type = metadata.file_type();

    if file_type.is_dir() {
        Ok(LayoutNode {
            name,
            kind: LayoutKind::Directory {
                children: read_dir_children(path)?,
            },
        })
    } else if file_type.is_file() {
        Ok(LayoutNode {
            name,
            kind: LayoutKind::File {
                data: fs::read(path)?,
            },
        })
    } else {
        Err(invalid_input(format!(
            "unsupported file type at `{}`; only directories and regular files are supported",
            path.display()
        )))
    }
}

fn validate_name(name: &str) -> io::Result<()> {
    if name == "." || name == ".." || name.contains('/') {
        return Err(invalid_input(format!("invalid path component `{name}`")));
    }

    if name.as_bytes().len() > 27 {
        return Err(invalid_input(format!(
            "path component `{name}` is longer than 27 bytes"
        )));
    }

    Ok(())
}

fn parse_layout_file(path: &Path) -> io::Result<LayoutNode> {
    let input = fs::read_to_string(path)?;
    let json: JsonLayoutNode = serde_json::from_str(&input)
        .map_err(|err| invalid_input(format!("invalid layout JSON: {err}")))?;
    let source_base = path.parent().unwrap_or_else(|| Path::new("."));

    layout_from_json(&json, "", source_base, true).map_err(invalid_input)
}

fn layout_from_json(
    json: &JsonLayoutNode,
    parent_path: &str,
    source_base: &Path,
    is_root: bool,
) -> Result<LayoutNode, String> {
    let raw_path = json
        .path
        .as_deref()
        .unwrap_or(if is_root { "/" } else { "" });
    let path = normalize_path(parent_path, raw_path, is_root)?;
    if !is_root && raw_path.starts_with('/') && absolute_parent(&path) != parent_path {
        return Err(format!(
            "absolute child path `{path}` is not contained by `{parent_path}`"
        ));
    }
    let name = if is_root {
        String::new()
    } else {
        path.rsplit('/')
            .next()
            .filter(|name| !name.is_empty())
            .ok_or_else(|| format!("invalid non-root path `{path}`"))?
            .to_string()
    };

    if name.as_bytes().len() > 27 {
        return Err(format!("path component `{name}` is longer than 27 bytes"));
    }

    match json.ty.as_str() {
        "directory" | "dir" => {
            let mut children = Vec::new();
            let mut names = BTreeSet::new();
            for child in &json.contains {
                let child = layout_from_json(child, &path, source_base, false)?;
                if !names.insert(child.name.clone()) {
                    return Err(format!(
                        "duplicate child name `{}` in directory `{path}`",
                        child.name
                    ));
                }
                children.push(child);
            }

            Ok(LayoutNode {
                name,
                kind: LayoutKind::Directory { children },
            })
        }
        "file" => {
            let data = if let Some(content) = json.content.as_deref() {
                content.as_bytes().to_vec()
            } else if let Some(content) = json.contents.as_deref() {
                content.as_bytes().to_vec()
            } else if let Some(source) = json.source.as_deref() {
                let source_path = Path::new(source);
                let source_path = if source_path.is_absolute() {
                    source_path.to_path_buf()
                } else {
                    source_base.join(source_path)
                };
                fs::read(&source_path).map_err(|err| {
                    format!(
                        "failed to read source file `{}`: {err}",
                        source_path.display()
                    )
                })?
            } else {
                Vec::new()
            };

            Ok(LayoutNode {
                name,
                kind: LayoutKind::File { data },
            })
        }
        other => Err(format!("unknown node type `{other}`")),
    }
}

fn normalize_path(parent_path: &str, raw_path: &str, is_root: bool) -> Result<String, String> {
    if is_root {
        if raw_path != "/" {
            return Err("root layout node must have path `/`".to_string());
        }
        return Ok(String::from("/"));
    }

    if raw_path.is_empty() || raw_path.ends_with('/') {
        return Err(format!("invalid path `{raw_path}`"));
    }

    let path = if raw_path.starts_with('/') {
        raw_path.to_string()
    } else if parent_path == "/" {
        format!("/{raw_path}")
    } else {
        format!("{parent_path}/{raw_path}")
    };

    if path
        .split('/')
        .any(|component| component == "." || component == "..")
    {
        return Err(format!("path `{path}` must not contain `.` or `..`"));
    }

    Ok(path)
}

fn absolute_parent(normalized_path: &str) -> &str {
    match normalized_path.rsplit_once('/') {
        Some(("", _)) => "/",
        Some((parent, _)) => parent,
        None => "",
    }
}

fn allocate_layout(layout: LayoutNode) -> Result<FsNode, String> {
    let mut next_inum = ROOT_INO;
    let mut next_data_block = DATA_BLOCK_START;
    allocate_node(layout, &mut next_inum, &mut next_data_block)
}

fn allocate_node(
    node: LayoutNode,
    next_inum: &mut u32,
    next_data_block: &mut u32,
) -> Result<FsNode, String> {
    if *next_inum >= NINODES {
        return Err(format!("too many inodes; image supports {}", NINODES - 1));
    }

    let inum = *next_inum;
    *next_inum += 1;

    match node.kind {
        LayoutKind::Directory { children } => {
            let mut allocated_children = Vec::with_capacity(children.len());
            for child in children {
                allocated_children.push(allocate_node(child, next_inum, next_data_block)?);
            }

            let dirent_count = 2 + allocated_children.len();
            let (data_blocks, indirect_block) = allocate_node_blocks(
                blocks_needed(dirent_count * std::mem::size_of::<DirEnt>()),
                next_data_block,
            )?;

            Ok(FsNode {
                inum,
                data_blocks,
                indirect_block,
                name: node.name,
                kind: FsNodeKind::Directory {
                    children: allocated_children,
                },
            })
        }
        LayoutKind::File { data } => {
            let (data_blocks, indirect_block) =
                allocate_node_blocks(blocks_needed(data.len()), next_data_block)?;
            Ok(FsNode {
                inum,
                data_blocks,
                indirect_block,
                name: node.name,
                kind: FsNodeKind::File { data },
            })
        }
    }
}

fn blocks_needed(bytes: usize) -> usize {
    if bytes == 0 {
        0
    } else {
        bytes.div_ceil(BLOCK_SIZE)
    }
}

fn allocate_node_blocks(
    count: usize,
    next_data_block: &mut u32,
) -> Result<(Vec<u32>, u32), String> {
    if count > MAX_DATA_BLOCKS {
        return Err(format!(
            "files and directories support at most {MAX_DATA_BLOCKS} data blocks"
        ));
    }

    let blocks = allocate_blocks(count, next_data_block)?;
    let indirect_block = if count > DIRECT_BLOCKS {
        allocate_blocks(1, next_data_block)?[0]
    } else {
        0
    };

    Ok((blocks, indirect_block))
}

fn allocate_blocks(count: usize, next_data_block: &mut u32) -> Result<Vec<u32>, String> {
    let end_block = next_data_block
        .checked_add(count as u32)
        .ok_or_else(|| "data block count overflow".to_string())?;
    if end_block > NBLOCKS {
        return Err(format!(
            "too many data blocks; image supports {NBLOCKS} blocks"
        ));
    }

    let blocks = (*next_data_block..end_block).collect();
    *next_data_block = end_block;
    Ok(blocks)
}

fn write_image(path: &str, root: &FsNode) -> io::Result<()> {
    let mut img = File::create(path)?;

    img.set_len((NBLOCKS as usize * BLOCK_SIZE) as u64)?;

    write_superblock(&mut img)?;
    write_inode_bitmap(&mut img, root)?;
    write_data_bitmap(&mut img, root)?;
    write_node(&mut img, root, ROOT_INO)?;

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
    let inode_size = std::mem::size_of::<INodeInner>() as u64;

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

fn write_inode_bitmap(img: &mut File, root: &FsNode) -> io::Result<()> {
    let mut bitmap = [0u8; BLOCK_SIZE];
    visit_nodes(root, &mut |node| set_bit(&mut bitmap, node.inum));

    write_at_block(img, INODE_BITMAP_BLOCK, &bitmap)
}

fn write_data_bitmap(img: &mut File, root: &FsNode) -> io::Result<()> {
    let mut bitmap = [0u8; BLOCK_SIZE];
    visit_nodes(root, &mut |node| {
        for block in &node.data_blocks {
            set_bit(&mut bitmap, block - DATA_BLOCK_START);
        }
        if node.indirect_block != 0 {
            set_bit(&mut bitmap, node.indirect_block - DATA_BLOCK_START);
        }
    });

    write_at_block(img, DATA_BITMAP_BLOCK, &bitmap)
}

fn visit_nodes(node: &FsNode, f: &mut impl FnMut(&FsNode)) {
    f(node);
    if let FsNodeKind::Directory { children } = &node.kind {
        for child in children {
            visit_nodes(child, f);
        }
    }
}

fn set_bit(bitmap: &mut [u8], bit: u32) {
    let byte = bit / 8;
    let off = bit % 8;
    bitmap[byte as usize] |= 1 << off;
}

fn write_node(img: &mut File, node: &FsNode, parent_inum: u32) -> io::Result<()> {
    write_indirect_block(img, node)?;

    match &node.kind {
        FsNodeKind::Directory { children } => {
            write_dir_inode(img, node, children)?;
            write_dir_blocks(img, node, parent_inum, children)?;

            for child in children {
                write_node(img, child, node.inum)?;
            }
        }
        FsNodeKind::File { data } => write_file(img, node, data)?,
    }

    Ok(())
}

fn write_dir_inode(img: &mut File, node: &FsNode, children: &[FsNode]) -> io::Result<()> {
    let direct_blocks = direct_blocks(node);

    let inode = INodeInner {
        ty: Type::Directory,
        link_count: 1,
        metadata: Metadata {
            sz: ((2 + children.len()) * std::mem::size_of::<DirEnt>()) as u32,
            dev: 0,
        },
        direct_blocks,
        indirect_block: node.indirect_block,
    };

    write_struct(img, inode_offset(node.inum), &inode)
}

fn write_dir_blocks(
    img: &mut File,
    node: &FsNode,
    parent_inum: u32,
    children: &[FsNode],
) -> io::Result<()> {
    let mut entries = Vec::with_capacity(2 + children.len());
    entries.push(dirent(node.inum, "."));
    entries.push(dirent(parent_inum, ".."));
    for child in children {
        entries.push(dirent(child.inum, &child.name));
    }

    let bytes = unsafe {
        std::slice::from_raw_parts(
            entries.as_ptr() as *const u8,
            std::mem::size_of_val(entries.as_slice()),
        )
    };

    for (block_idx, block) in node.data_blocks.iter().enumerate() {
        let start = block_idx * BLOCK_SIZE;
        let end = usize::min(start + BLOCK_SIZE, bytes.len());
        write_at_block(img, *block, &bytes[start..end])?;
    }

    Ok(())
}

fn write_file(img: &mut File, node: &FsNode, data: &[u8]) -> io::Result<()> {
    let direct_blocks = direct_blocks(node);

    let inode = INodeInner {
        ty: Type::File,
        link_count: 1,
        metadata: Metadata {
            dev: 0,
            sz: data.len() as u32,
        },
        direct_blocks,
        indirect_block: node.indirect_block,
    };

    write_struct(img, inode_offset(node.inum), &inode)?;

    for (block_idx, block) in node.data_blocks.iter().enumerate() {
        let start = block_idx * BLOCK_SIZE;
        let end = usize::min(start + BLOCK_SIZE, data.len());
        write_at_block(img, *block, &data[start..end])?;
    }

    Ok(())
}

fn direct_blocks(node: &FsNode) -> [u32; DIRECT_BLOCKS] {
    let mut direct_blocks = [0; DIRECT_BLOCKS];
    let direct_count = node.data_blocks.len().min(DIRECT_BLOCKS);
    direct_blocks[..direct_count].copy_from_slice(&node.data_blocks[..direct_count]);
    direct_blocks
}

fn write_indirect_block(img: &mut File, node: &FsNode) -> io::Result<()> {
    if node.indirect_block == 0 {
        return Ok(());
    }

    let indirect_data_blocks = &node.data_blocks[DIRECT_BLOCKS..];
    debug_assert!(indirect_data_blocks.len() <= INDIRECT_BLOCK_ENTRIES);
    let mut block = [0; BLOCK_SIZE];
    for (index, data_block) in indirect_data_blocks.iter().enumerate() {
        let offset = index * std::mem::size_of::<u32>();
        block[offset..offset + std::mem::size_of::<u32>()]
            .copy_from_slice(&data_block.to_ne_bytes());
    }

    write_at_block(img, node.indirect_block, &block)
}

fn dirent(inum: u32, name: &str) -> DirEnt {
    let mut out = DirEnt {
        inum,
        name_len: name.len() as u8,
        name: [0u8; 27],
    };

    let bytes = name.as_bytes();
    assert!(bytes.len() <= out.name.len());

    out.name[..bytes.len()].copy_from_slice(bytes);

    out
}

fn invalid_input(err: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, err.into())
}
