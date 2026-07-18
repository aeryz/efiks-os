// long sys_openat(int dfd, const char __user *filename, int flags, umode_t
// mode);
pub const SYS_OPEN: usize = 56;

// long sys_close(unsigned int fd);
pub const SYS_CLOSE: usize = 57;

// long sys_read(unsigned int fd, char __user *buf, size_t count);
pub const SYS_READ: usize = 63;
//  sys_write(unsigned int fd, const char __user *buf, size_t count);
pub const SYS_WRITE: usize = 64;

// long sys_exit(int error_code);
pub const SYS_EXIT: usize = 93;

// long sys_nanosleep(struct __kernel_timespec __user *rqtp, struct
// __kernel_timespec __user *rmtp);
// TODO(aeryz): change the impl to nanosleep
pub const SYS_SLEEP_MS: usize = 101;
pub const SYS_SHUTDOWN: usize = 210;
// long sys_brk(unsigned long brk);
pub const SYS_BRK: usize = 214;
// long sys_wait4(pid_t pid, int __user *stat_addr, int options, struct rusage
// __user *ru);
pub const SYS_WAIT: usize = 260;

// TODO(aeryz): temporary until we have fork
pub const SYS_SPAWN: usize = 6;
