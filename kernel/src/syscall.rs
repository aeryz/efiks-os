use alloc::vec::Vec;

use crate::{
    Arch,
    arch::{TrapFrame, TrapFrameOf, syscall},
    error::Error,
    mm::{UserBuf, UserBufMut, UserPtr, VirtAddr},
    sched,
    task::{self, RawWaitStatus},
};
use efiks_types::Errno;

pub fn dispatch_syscall(tf: &mut TrapFrameOf<Arch>) {
    let syscall_number = tf.get_syscall();
    let ret = match do_dispatch_syscall(syscall_number, tf) {
        Ok(ret) => ret,
        Err(err) => -(Into::<Errno>::into(err) as isize),
    };

    tf.set_syscall_return_value(ret);
}

fn do_dispatch_syscall(syscall_number: usize, tf: &mut TrapFrameOf<Arch>) -> Result<isize, Error> {
    match syscall_number {
        syscall::SYS_OPEN => {
            let path = UserBuf::new(tf.get_arg::<0>()).ok_or(Error::InvalidArgs)?;
            let flags = tf.get_arg_as::<1, u32>()?;
            syscall_open::sys_open(path, flags).map(|fd| fd as isize)
        }
        syscall::SYS_CLOSE => {
            let fd = tf.get_arg_as::<0, u32>()?;
            sys_close(fd).map(|_| 0)
        }
        syscall::SYS_READ => {
            let fd = tf.get_arg_as::<0, u32>()?;
            let buf = UserBufMut::new(tf.get_arg::<1>()).ok_or(Error::InvalidArgs)?;
            let count = tf.get_arg::<2>();
            sys_read(fd, buf, count).map(|n| n as isize)
        }
        syscall::SYS_WRITE => {
            let fd = tf.get_arg_as::<0, u32>()?;
            let buf = UserBuf::new(tf.get_arg::<1>()).ok_or(Error::InvalidArgs)?;
            let count = tf.get_arg::<2>();

            sys_write(fd, buf, count).map(|n| n as isize)
        }
        syscall::SYS_EXIT => {
            let exit_code = tf.get_arg_as::<0, i8>()?;
            sys_exit(exit_code);
            Ok(0)
        }
        syscall::SYS_SLEEP_MS => {
            let time_ms = tf.get_arg::<0>();
            sys_sleep_ms(time_ms);
            Ok(0)
        }
        // TODO(aeryz): Shouldn't this supposed to be `Brk`?
        syscall::SYS_BRK => {
            let brk = tf.get_arg::<0>() as usize;
            sys_brk(brk).map(|n| n as isize)
        }
        syscall::SYS_WAIT => {
            let out_wstatus = UserPtr::<RawWaitStatus>::new(tf.get_arg::<0>());
            sys_wait(out_wstatus).map(|p| p.raw() as isize)
        }
        syscall::SYS_SPAWN => {
            let out_pid = UserPtr::<task::Pid>::new(tf.get_arg::<0>());
            let path = UserBuf::new(tf.get_arg::<1>()).ok_or(Error::InvalidArgs)?;
            let argv: UserPtr<UserPtr<u8>> = UserPtr::new(tf.get_arg::<2>());
            sys_spawn(path, argv, out_pid).map(|_| 0)
        }
        n => {
            panic!("unsupported syscall: {n}")
        }
    }
}

mod syscall_open {
    use bitflags::bitflags;

    use super::*;

    bitflags! {
        #[repr(transparent)]
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub struct Flags: u32 {
            const RDONLY    = 0o0000000;
            const WRONLY    = 0o0000001;
            const RDWR      = 0o0000002;
            const ACCMODE   = 0o0000003;

            const CREAT     = 0o0000100;
            const EXCL      = 0o0000200;
            const NOCTTY    = 0o0000400;
            const TRUNC     = 0o0001000;
            const APPEND    = 0o0002000;
            const NONBLOCK  = 0o0004000;
            const DSYNC     = 0o0010000;
            const ASYNC     = 0o0020000;
            const DIRECT    = 0o0040000;
            const LARGEFILE = 0o0100000;
            const DIRECTORY = 0o0200000;
            const NOFOLLOW  = 0o0400000;
            const NOATIME   = 0o1000000;
            const CLOEXEC   = 0o2000000;

            // Linux defines O_SYNC = __O_SYNC | O_DSYNC.
            const SYNC      = 0o4010000;

            const PATH      = 0o10000000;
            const TMPFILE   = 0o20200000;
        }
    }

    // TODO(aeryz): make this linux-compatible
    /// ```c
    /// long sys_openat(
    ///     int dfd,
    ///     const char __user *filename,
    ///     int flags,
    ///     umode_t mode
    /// );
    /// ```
    pub(super) fn sys_open(path: UserBuf, flags: u32) -> Result<usize, Error> {
        let flags = Flags::from_bits(flags).ok_or(Error::InvalidArgs)?;

        let mut kpath = [0; vfs::MAX_FILE_PATH_LENGTH];
        let n_path = unsafe {
            path.copy_from_user_until(&mut kpath, |b| b == 0)
                .ok_or(Error::InvalidArgs)?
        };

        let kpath = &kpath[0..n_path];
        if kpath.is_empty() {
            return Err(Error::InvalidArgs);
        }

        let add_file = |file: vfs::File| -> usize {
            let ctx = sched::load_core_ctx();
            ctx.current_task.file_table.lock().add_file(file)
        };

        match crate::vfs::open(kpath) {
            Ok(file) => {
                if flags.contains(Flags::CREAT | Flags::EXCL) {
                    return Err(Error::Errno(Errno::EExist));
                }

                Ok(add_file(file))
            }
            Err(err) if err == vfs::VfsError::NotFound => {
                if !flags.contains(Flags::CREAT) {
                    return Err(err.into());
                }

                let file = crate::vfs::create(kpath)?;
                Ok(add_file(file))
            }
            Err(err) => Err(err.into()),
        }
    }
}

/// ```c
/// long sys_close(unsigned int fd);
/// ```
pub fn sys_close(fd: u32) -> Result<(), Error> {
    let _ = sched::load_core_ctx()
        .current_task
        .file_table
        .lock()
        .close_file(fd as usize)
        .ok_or(Errno::EBadF)?;
    Ok(())
}

/// ```c
/// long sys_read(unsigned int fd, char __user *buf, size_t count);
/// ```
fn sys_read(fd: u32, buf: UserBufMut, count: usize) -> Result<usize, Error> {
    if count == 0 {
        core::hint::cold_path();
        return Ok(0);
    }

    let ctx = sched::load_core_ctx();

    let file = ctx
        .current_task
        .file_table
        .lock()
        .get_file(fd)
        .ok_or(Error::NotFound)?;

    let mut kbuf = Vec::new();
    kbuf.resize(count, 0u8);

    let count = file.lock().read(&mut kbuf)?;

    unsafe {
        buf.copy_into_user(&kbuf[0..count]);
    }

    Ok(count)
}

/// ```c
/// long sys_write(unsigned int fd, const char __user *buf, size_t count);
/// ```
fn sys_write(fd: u32, buf: UserBuf, count: usize) -> Result<usize, Error> {
    if count == 0 {
        core::hint::cold_path();
        return Ok(0);
    }

    let ctx = sched::load_core_ctx();

    let file = ctx
        .current_task
        .file_table
        .lock()
        .get_file(fd)
        .ok_or(Error::NotFound)?;

    // TODO(aeryz): we can have either a kernel-wide or a syscall-wide memory pool
    // to not allocate resources that will be allocated and free'd constantly.
    // Tbf, I'm probably talking about a new allocator for this purpose only.
    let mut kbuf = Vec::new();
    kbuf.resize(count, 0);

    unsafe {
        buf.copy_from_user(&mut kbuf);
    }

    let count = file.lock().write(&kbuf[0..count])?;

    Ok(count)
}

/// ```c
/// long sys_exit(int error_code);
/// ```
fn sys_exit(exit_code: i8) {
    let task = &sched::load_core_ctx().current_task;
    task::exit(task, exit_code);
}

// TODO(aeryz): make this linux-compatible
/// ```c
/// long sys_nanosleep(
///     struct __kernel_timespec __user *rqtp,
///     struct __kernel_timespec __user *rmtp
/// );
/// ```
fn sys_sleep_ms(time_ms: usize) {
    // TODO(aeryz): task subsystem should know how to put this into sleep.
    sched::sleep_current_task(time_ms);
}

/// ```c
/// long sys_brk(unsigned long brk);
/// ```
fn sys_brk(brk: usize) -> Result<usize, Error> {
    let task = &sched::load_core_ctx().current_task;
    let new_brk = task.mm.brk(VirtAddr::new(brk))?.raw();
    log::debug!("new brk: 0x{new_brk:x}");
    Ok(new_brk)
}

// TODO(aeryz): make this linux-compatible
/// ```c
/// long sys_wait4(
///     pid_t pid,
///     int __user *stat_addr,
///     int options,
///     struct rusage __user *ru
/// );
/// ```
fn sys_wait(out_wstatus: UserPtr<RawWaitStatus>) -> Result<task::Pid, Error> {
    let task = &sched::load_core_ctx().current_task;
    // TODO(aeryz): we should get the exit code of the child?
    let (pid, exit_code) = task::wait(task)?;

    let raw_stat: RawWaitStatus = task::WaitStatus::Exited(exit_code).into();
    unsafe {
        out_wstatus.copy_into_user(&raw_stat);
    }

    Ok(pid)
}

fn sys_spawn(
    path: UserBuf,
    argv: UserPtr<UserPtr<u8>>,
    out_pid: UserPtr<task::Pid>,
) -> Result<(), Error> {
    let mut kpath = [0; vfs::MAX_FILE_PATH_LENGTH];
    let n_path = unsafe {
        path.copy_from_user_until(&mut kpath, |b| b == 0)
            .ok_or(Error::InvalidArgs)?
    };

    if n_path == 0 {
        return Err(Error::InvalidArgs);
    }

    let mut kargv = Vec::new();

    let mut argv_iter = argv.iter();
    while let Some(current_arg) = argv_iter.next() {
        if current_arg.is_null() {
            break;
        }

        // TODO(aeryz): define a max
        let mut karg = [0; 128];
        let n_read = unsafe {
            current_arg
                .copy_from_user_many_until(&mut karg, |item| *item == 0)
                .ok_or(Error::InvalidArgs)?
        };
        kargv.push(karg[0..n_read].to_vec());
    }

    let args: Vec<&[u8]> = kargv.iter().map(|v| v.as_slice()).collect();
    let pid = task::spawn(
        &kpath[0..n_path],
        &args,
        Some(&sched::load_core_ctx().current_task),
    )?;

    unsafe {
        out_pid.copy_into_user(&pid);
    }

    Ok(())
}
