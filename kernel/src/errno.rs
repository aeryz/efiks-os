#[repr(isize)]
pub enum Errno {
    /// No such file or directory
    ENoEnt = 2,
    /// I/O error
    EIO = 5,
    /// Exec format error
    ENoExec = 8,
    /// Cannot allocate memory
    ENoMem = 12,
    /// Bad address
    EFault = 14,
    /// Function not implemented
    ENoSys = 38,
    /// Invalid argument
    EInval = 22,
    /// Value too large for defined data type
    EOverflow = 75,
}
