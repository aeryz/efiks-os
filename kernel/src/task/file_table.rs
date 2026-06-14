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

    pub fn get_file(&self, fd: usize) -> Option<FileRef> {
        self.0.get(fd)?.clone()
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
