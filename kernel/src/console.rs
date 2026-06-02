use alloc::sync::Arc;
use vfs::{File, VNode};

pub struct Console {}

impl Console {
    pub fn as_file() -> File {
        File {
            inode: Arc::new(ConsoleVNode),
            offset: 0,
        }
    }
}

struct ConsoleVNode;

impl VNode for ConsoleVNode {
    fn open(&self, _path: &[u8]) -> vfs::VfsResult<vfs::File> {
        Err(vfs::VfsError::Fs)
    }

    fn read(&self, _offset: usize, _buf: &mut [u8]) -> vfs::VfsResult<usize> {
        Err(vfs::VfsError::Fs)
    }

    fn write(&self, _offset: usize, buf: &[u8]) -> vfs::VfsResult<usize> {
        crate::printk(buf);
        Ok(buf.len())
    }

    fn sz(&self) -> usize {
        0
    }
}
