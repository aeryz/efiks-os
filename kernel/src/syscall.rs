use alloc::{sync::Arc, vec::Vec};

use crate::{
    Arch,
    arch::{Architecture, TrapFrame, TrapFrameOf},
    error::Error,
    mm::{UserBuf, UserBufMut, UserPtr, VirtAddr},
    percpu, sched, task,
};

#[allow(unused)]
#[repr(usize)]
pub enum Syscall {
    Write = 1,
    Read,
    SleepMs,
    Shutdown,
    Exit,
    Spawn,
    Wait,
    // Match the linux kernel for Zig's `BrkAllocator` compatibility
    Sbrk = 214,
    End,
}

// TODO(aeryz): We don't want to implement the syscalls here. They should
// directly be implemented in their respective subsystem.
#[unsafe(no_mangle)]
#[inline(never)]
pub fn dispatch_syscall(tf: &mut TrapFrameOf<Arch>) {
    let syscall_number = tf.get_syscall();
    let syscall = if syscall_number < Syscall::End as usize {
        unsafe { core::mem::transmute::<usize, Syscall>(syscall_number) }
    } else {
        return;
    };

    match syscall {
        Syscall::Write => {
            let fd = tf.get_arg::<0>();
            let buf = UserBuf::new(tf.get_arg::<1>())
                .ok_or(Error::InvalidArgs)
                .unwrap();
            let count = tf.get_arg::<2>();
            let _ = do_syscall_write(fd, buf, count);
        }
        Syscall::Read => {
            let fd = tf.get_arg::<0>();
            let buf = UserBufMut::new(tf.get_arg::<1>())
                .ok_or(Error::InvalidArgs)
                .unwrap();
            let count = tf.get_arg::<2>();
            let _ = do_syscall_read(fd, buf, count);
        }
        Syscall::Shutdown => do_syscall_shutdown(),
        Syscall::SleepMs => {
            let time_ms = tf.get_arg::<0>();
            do_syscall_sleep_ms(time_ms);
        }
        Syscall::Spawn => {
            let pid_ptr = UserPtr::<task::Pid>::new(tf.get_arg::<0>());
            let Some(path_buf) = UserBuf::new(tf.get_arg::<1>()) else {
                tf.set_syscall_return_value(usize::MAX);
                return;
            };
            let argv_ptr: UserPtr<usize> = UserPtr::new(tf.get_arg::<2>());

            if pid_ptr.is_null() || argv_ptr.is_null() {
                tf.set_syscall_return_value(usize::MAX);
                return;
            }

            let mut path = [0; vfs::MAX_FILE_PATH_LENGTH];
            let Some(n_path) = (unsafe { path_buf.copy_from_user_until(&mut path, |b| b == 0) })
            else {
                tf.set_syscall_return_value(usize::MAX);
                return;
            };
            if n_path == 0 {
                tf.set_syscall_return_value(usize::MAX);
                return;
            }

            let mut arg = Vec::new();
            // TODO(aeryz): this is temporary max
            arg.resize(256, 0);

            let mut argv_storage = Vec::new();
            let mut i = 0;
            loop {
                let Some(cur_arg_ptr) = argv_ptr.offset(i) else {
                    tf.set_syscall_return_value(usize::MAX);
                    return;
                };
                let mut arg_addr = 0;
                unsafe {
                    cur_arg_ptr.copy_from_user(&mut arg_addr);
                }
                if arg_addr == 0 {
                    break;
                }

                let Some(arg_ptr) = UserBuf::new(arg_addr) else {
                    tf.set_syscall_return_value(usize::MAX);
                    return;
                };
                let Some(n_copied) =
                    (unsafe { arg_ptr.copy_from_user_until(&mut arg, |b| b == 0) })
                else {
                    tf.set_syscall_return_value(usize::MAX);
                    return;
                };
                argv_storage.push(arg[0..n_copied].to_vec());
                i += 1;
            }

            let mut argv = Vec::new();
            for arg in &argv_storage {
                argv.push(arg.as_slice());
            }

            log::info!("spawn path is: {}", unsafe {
                str::from_utf8_unchecked(&path)
            });

            let this_ctx = unsafe {
                Arch::load_this_cpu_ctx::<percpu::PerCoreContext>()
                    .as_mut()
                    .unwrap()
            };

            unsafe {
                pid_ptr.copy_into_user(&match task::spawn(
                    &path[..n_path],
                    &argv,
                    Some(&this_ctx.current_task),
                ) {
                    Ok(pid) => pid,
                    Err(e) => {
                        log::error!("couldn't spawn due to {e:?}");
                        tf.set_syscall_return_value(usize::MAX);
                        return;
                    }
                });
            }

            tf.set_syscall_return_value(0);
        }
        Syscall::Exit => {
            let exit_code = tf.get_arg::<0>() as i32;
            let _ = do_syscall_exit(exit_code);
        }
        Syscall::Wait => {
            let _ = do_syscall_wait();
        }
        // TODO(aeryz): Shouldn't this supposed to be `Brk`?
        Syscall::Sbrk => {
            let brk = tf.get_arg::<0>() as usize;
            let _ = do_syscall_brk(brk);
        }
        _ => panic!(
            "Unhandled syscall. Yes, this is kernel failure because want to stop and take care of it at this time"
        ),
    }
}

fn load_core_ctx<'a>() -> &'a percpu::PerCoreContext {
    unsafe {
        Arch::load_this_cpu_ctx::<percpu::PerCoreContext>()
            .as_ref()
            .unwrap()
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

    let ctx = load_core_ctx();

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

    let ctx = load_core_ctx();

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
    let task = &load_core_ctx().current_task;
    task::exit(task, exit_code);
}

fn do_syscall_wait() -> Result<i32, Error> {
    let task = &load_core_ctx().current_task;
    // TODO(aeryz): we should get the exit code of the child?
    task::wait(task).map(|_| 0)
}

fn do_syscall_brk(brk: usize) -> Result<usize, Error> {
    let task = &load_core_ctx().current_task;
    let new_brk = task.mm.brk(VirtAddr::new(brk)).unwrap().raw();
    log::debug!("new brk: 0x{new_brk:x}");
    Ok(new_brk)
}
