use core::ops::Deref;

use alloc::{sync::Arc, vec::Vec};
use ksync::SpinLock;
use vfs::File;

use crate::console::Console;

pub struct FileTable(Vec<Option<FileRef>>);

impl FileTable {
    pub fn init() -> Self {
        Self(alloc::vec![
            Some(FileRef::new(Console::as_file())),
            Some(FileRef::new(Console::as_file())),
            Some(FileRef::new(Console::as_file())),
        ])
    }

    // TODO(aeryz): Ik linux reuses fds but I don't like it. Let's have it for now
    // but I'm probably gonna change it. The problem is that libc uses int fds so
    // I'll probably need to pack the index + generation to the int.
    pub fn add_file(&mut self, file: File) -> usize {
        if let Some((i, slot)) = self.0.iter_mut().enumerate().find_map(|(i, slot)| {
            if slot.is_none() {
                Some((i, slot))
            } else {
                None
            }
        }) {
            *slot = Some(FileRef::new(file));
            i
        } else {
            self.0.push(Some(FileRef::new(file)));
            self.0.len() - 1
        }
    }

    pub fn get_file(&self, fd: u32) -> Option<FileRef> {
        self.0.get(fd as usize)?.clone()
    }

    pub fn close_file(&mut self, fd: usize) -> Option<FileRef> {
        self.0.get_mut(fd)?.take()
    }

    pub fn destroy(&mut self) {
        // `Vec::new` has an initial capacity of 0
        self.0 = Vec::new();
    }
}

#[derive(Clone)]
pub struct FileRef(Arc<SpinLock<File>>);

impl FileRef {
    pub fn new(file: File) -> Self {
        Self(Arc::new(SpinLock::new(file)))
    }
}

impl Deref for FileRef {
    type Target = Arc<SpinLock<File>>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
