use alloc::vec::Vec;

use crate::{
    Arch,
    arch::{Architecture, TrapFrame, TrapFrameOf},
    error::Error,
    mm::{UserBuf, UserBufMut, UserPtr, VirtAddr},
    percpu, sched,
    task::{self, Task},
};
use efiks_types::Errno;

pub(crate) const SYS_WRITE: usize = 1;
pub(crate) const SYS_READ: usize = 2;
pub(crate) const SYS_SLEEP_MS: usize = 3;
pub(crate) const SYS_SHUTDOWN: usize = 4;
pub(crate) const SYS_EXIT: usize = 5;
pub(crate) const SYS_SPAWN: usize = 6;
pub(crate) const SYS_WAIT: usize = 7;
// Match the linux kernel for Zig's `BrkAllocator` compatibility.
pub(crate) const SYS_BRK: usize = 214;

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
        SYS_WRITE => {
            let fd = tf.get_arg::<0>();
            let buf = UserBuf::new(tf.get_arg::<1>()).ok_or(Error::InvalidArgs)?;
            let count = tf.get_arg::<2>();

            do_syscall_write(fd, buf, count).map(|n| n as isize)
        }
        SYS_READ => {
            let fd = tf.get_arg::<0>();
            let buf = UserBufMut::new(tf.get_arg::<1>()).ok_or(Error::InvalidArgs)?;
            let count = tf.get_arg::<2>();
            do_syscall_read(fd, buf, count).map(|n| n as isize)
        }
        SYS_SHUTDOWN => do_syscall_shutdown(),
        SYS_SLEEP_MS => {
            let time_ms = tf.get_arg::<0>();
            do_syscall_sleep_ms(time_ms);
            Ok(0)
        }
        SYS_SPAWN => {
            let out_pid = UserPtr::<task::Pid>::new(tf.get_arg::<0>());
            let path = UserBuf::new(tf.get_arg::<1>()).ok_or(Error::InvalidArgs)?;
            let argv: UserPtr<UserPtr<u8>> = UserPtr::new(tf.get_arg::<2>());
            do_syscall_spawn(path, argv, out_pid).map(|_| 0)
        }
        SYS_EXIT => {
            let exit_code = tf.get_arg::<0>() as i32;
            do_syscall_exit(exit_code);
            Ok(0)
        }
        SYS_WAIT => do_syscall_wait().map(|n| n as isize),
        // TODO(aeryz): Shouldn't this supposed to be `Brk`?
        SYS_BRK => {
            let brk = tf.get_arg::<0>() as usize;
            do_syscall_brk(brk).map(|n| n as isize)
        }
        _ => Err(Error::NoSys),
    }
}

fn do_syscall_write(
    // TODO(aeryz): what's gonna be fd type?
    fd: usize,
    buf: UserBuf,
    count: usize,
) -> Result<usize, Error> {
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

fn do_syscall_read(fd: usize, buf: UserBufMut, count: usize) -> Result<usize, Error> {
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

fn do_syscall_shutdown() -> ! {
    loop {
        Arch::halt();
        core::hint::spin_loop();
    }
}

fn do_syscall_sleep_ms(time_ms: usize) {
    // TODO(aeryz): task subsystem should know how to put this into sleep.
    sched::sleep_current_task(time_ms);
}

fn do_syscall_exit(exit_code: i32) {
    let task = &sched::load_core_ctx().current_task;
    task::exit(task, exit_code);
}

fn do_syscall_wait() -> Result<i32, Error> {
    let task = &sched::load_core_ctx().current_task;
    // TODO(aeryz): we should get the exit code of the child?
    task::wait(task).map(|_| 0)
}

fn do_syscall_brk(brk: usize) -> Result<usize, Error> {
    let task = &sched::load_core_ctx().current_task;
    let new_brk = task.mm.brk(VirtAddr::new(brk))?.raw();
    log::debug!("new brk: 0x{new_brk:x}");
    Ok(new_brk)
}

fn do_syscall_spawn(
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
