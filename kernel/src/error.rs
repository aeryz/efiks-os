use crate::exec;

#[derive(Debug)]
pub enum Error {
    Vfs(vfs::VfsError),
    Elf(exec::elf::Error),
    Unaligned,
    Overflow,
    /// Errors that are undecided yet
    Todo,
}

impl From<vfs::VfsError> for Error {
    fn from(value: vfs::VfsError) -> Self {
        Self::Vfs(value)
    }
}

impl From<exec::elf::Error> for Error {
    fn from(value: exec::elf::Error) -> Self {
        Self::Elf(value)
    }
}
