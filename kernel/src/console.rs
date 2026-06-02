use alloc::sync::Arc;
use vfs::{File, VNode};

use crate::{arch::plic, driver, sched};

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

    fn read(&self, _offset: usize, buf: &mut [u8]) -> vfs::VfsResult<usize> {
        let mut i = 0;
        while i < buf.len() {
            loop {
                match driver::uart::try_get_char() {
                    Some(c) => {
                        log::trace!("read something, not scheduling\n");
                        if c == b'\n' || c == b'\r' {
                            return Ok(i);
                        }
                        buf[i] = c;
                        i += 1;
                        break;
                    }
                    None => {
                        log::trace!("couldn't read anything, scheduling\n");
                        // TODO(aeryz): arch specific, remove
                        sched::block_on_external_irq(plic::UART0_IRQ);
                    }
                }
            }
        }

        Ok(i)
    }

    fn write(&self, _offset: usize, buf: &[u8]) -> vfs::VfsResult<usize> {
        crate::printk(buf);
        Ok(buf.len())
    }

    fn sz(&self) -> usize {
        0
    }
}
