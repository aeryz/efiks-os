pub const SYS_IOCTL: usize = 29;
pub const SYS_OPEN: usize = 56;
pub const SYS_CLOSE: usize = 57;
pub const SYS_READ: usize = 63;
pub const SYS_WRITE: usize = 64;
pub const SYS_READV: usize = 65;
pub const SYS_WRITEV: usize = 66;
pub const SYS_PREADV: usize = 69;
pub const SYS_PWRITEV: usize = 70;
pub const SYS_EXIT: usize = 93;
// TODO(aeryz): change the impl to nanosleep
pub const SYS_SLEEP_MS: usize = 101;
pub const SYS_GETTID: usize = 178;
pub const SYS_BRK: usize = 214;
pub const SYS_WAIT: usize = 260;
// TODO(aeryz): temporary until we have fork
pub const SYS_SPAWN: usize = 6;
