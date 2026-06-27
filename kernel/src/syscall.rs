use core::ptr;

use alloc::vec::Vec;

use crate::{
    Arch,
    arch::{Architecture, TrapFrame, TrapFrameOf},
    mm::{UserPtr, VirtAddr},
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
            let buf_ptr = UserPtr::new(tf.get_arg::<1>());
            let count = tf.get_arg::<2>();

            // TODO(aeryz): we want to set error when buf_ptr == NULL?
            if buf_ptr.is_null() || count == 0 {
                tf.set_syscall_return_value(0);
                return;
            }

            let this_ctx = unsafe {
                Arch::load_this_cpu_ctx::<percpu::PerCoreContext>()
                    .as_ref()
                    .unwrap()
            };

            let file = {
                let file_table = this_ctx.current_task.file_table.lock();
                file_table.get_file(fd)
            };

            let Some(file) = file else {
                tf.set_syscall_return_value(0);
                return;
            };

            let mut buf = Vec::new();
            buf.resize(count, 0);

            let n_buf = copy_from_user(buf_ptr, buf.as_mut_ptr(), usize::MAX).unwrap();

            let count = file.lock().write(&buf[0..n_buf]).unwrap_or(usize::MAX);

            tf.set_syscall_return_value(count);
        }
        Syscall::Read => {
            let fd = tf.get_arg::<0>();
            let user_buf = UserPtr::new(tf.get_arg::<1>());
            let count = tf.get_arg::<2>();

            let this_ctx = unsafe {
                Arch::load_this_cpu_ctx::<percpu::PerCoreContext>()
                    .as_ref()
                    .unwrap()
            };

            let file = {
                let file_table = this_ctx.current_task.file_table.lock();
                file_table.get_file(fd)
            };

            let Some(file) = file else {
                tf.set_syscall_return_value(0);
                return;
            };

            let mut buf = Vec::new();
            buf.resize(count, 0u8);

            let count = file.lock().read(&mut buf).unwrap_or(usize::MAX);
            copy_into_user(&buf[0..count], user_buf).unwrap();

            tf.set_syscall_return_value(count);
        }
        Syscall::Shutdown => loop {
            Arch::halt();
            core::hint::spin_loop();
        },
        Syscall::SleepMs => {
            let time_ms = tf.get_arg::<0>();
            sched::sleep_current_task(time_ms);
        }
        Syscall::Spawn => {
            let pid_ptr = tf.get_arg::<0>() as *mut task::Pid;
            let path_ptr = UserPtr::new(tf.get_arg::<1>());
            let argv_ptr = tf.get_arg::<2>() as *const *const u8;

            if pid_ptr == ptr::null_mut() || path_ptr.is_null() || argv_ptr == ptr::null() {
                tf.set_syscall_return_value(usize::MAX);
                return;
            }

            let mut path = [0; vfs::MAX_FILE_PATH_LENGTH];
            let n_path =
                copy_from_user(path_ptr, path.as_mut_ptr(), vfs::MAX_FILE_PATH_LENGTH).unwrap();
            if n_path == 0 {
                tf.set_syscall_return_value(usize::MAX);
                return;
            }

            let mut argv_storage = Vec::new();
            let mut i = 0;
            loop {
                let arg_ptr = unsafe { *argv_ptr.add(i) };
                if arg_ptr == ptr::null() {
                    break;
                }

                let mut arg = Vec::new();
                arg.resize(strlen_user(arg_ptr), 0);
                // copy_from_user(arg_ptr, arg.as_mut_ptr(), arg.len()).unwrap();
                argv_storage.push(arg);
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
                *pid_ptr = match task::spawn(&path[..n_path], &argv, Some(&this_ctx.current_task)) {
                    Ok(pid) => pid,
                    Err(e) => {
                        log::error!("couldn't spawn due to {e:?}");
                        tf.set_syscall_return_value(usize::MAX);
                        return;
                    }
                };
            }

            tf.set_syscall_return_value(0);
        }
        Syscall::Exit => {
            let exit_code = tf.get_arg::<0>() as i32;

            let task = unsafe {
                &Arch::load_this_cpu_ctx::<percpu::PerCoreContext>()
                    .as_ref()
                    .unwrap()
                    .current_task
            };
            task::exit(task, exit_code);
            tf.set_syscall_return_value(0);
        }
        Syscall::Wait => {
            let task = unsafe {
                &Arch::load_this_cpu_ctx::<percpu::PerCoreContext>()
                    .as_ref()
                    .unwrap()
                    .current_task
            };

            let ret = match task::wait(task) {
                Ok(_) => 0,
                Err(_) => usize::MAX,
            };

            tf.set_syscall_return_value(ret);
        }
        Syscall::Sbrk => {
            let brk = tf.get_arg::<0>() as usize;
            let task = unsafe {
                &Arch::load_this_cpu_ctx::<percpu::PerCoreContext>()
                    .as_ref()
                    .unwrap()
                    .current_task
            };

            let new_brk = task.mm.brk(VirtAddr::new(brk)).unwrap().raw();
            log::info!("new brk: 0x{new_brk:x}");
            tf.set_syscall_return_value(new_brk);
        }
        _ => unreachable!(),
    }
}

// TODO: maybe put this in `mm`?
/// Copies from user buffer until it sees NULL.
pub fn copy_from_user(user_ptr: UserPtr, dest_ptr: *mut u8, max: usize) -> Option<usize> {
    for i in 0..max {
        unsafe {
            let b = *(user_ptr.offset_by(i as isize)?.raw() as *const u8);
            if b == 0 {
                return Some(i);
            }
            *dest_ptr.add(i) = b;
        }
    }

    Some(0)
}

/// Copies into user
pub fn copy_into_user(buf: &[u8], user_ptr: UserPtr) -> Option<usize> {
    for (i, b) in buf.iter().enumerate() {
        unsafe {
            *(user_ptr.offset_by(i as isize)?.raw() as *mut u8) = *b;
        }
    }

    Some(0)
}

fn strlen_user(user_ptr: *const u8) -> usize {
    let mut i = 0;
    loop {
        let b = unsafe { *user_ptr.add(i) };
        if b == 0 {
            return i;
        }

        i += 1;
    }
}
