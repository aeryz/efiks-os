use core::ptr;

use alloc::vec::Vec;

use crate::{
    Arch,
    arch::{Architecture, TrapFrame, TrapFrameOf},
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
    Sbrk,
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
            let buf_ptr = tf.get_arg::<1>() as *const u8;
            let count = tf.get_arg::<2>();

            // TODO(aeryz): we want to set error when buf_ptr == NULL?
            if buf_ptr == core::ptr::null() || count == 0 {
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

            let n_buf = copy_from_user(buf_ptr, buf.as_mut_ptr(), usize::MAX);

            let count = file.lock().write(&buf[0..n_buf]).unwrap_or(usize::MAX);

            tf.set_syscall_return_value(count);
        }
        Syscall::Read => {
            let fd = tf.get_arg::<0>();
            let buf = tf.get_arg::<1>() as *mut u8;
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

            let count = file
                .lock()
                .read(unsafe { core::slice::from_raw_parts_mut(buf, count) })
                .unwrap_or(usize::MAX);

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
            let path_ptr = tf.get_arg::<1>() as *const u8;
            let argv_ptr = tf.get_arg::<2>() as *const *const u8;

            if pid_ptr == ptr::null_mut() || path_ptr == ptr::null() || argv_ptr == ptr::null() {
                tf.set_syscall_return_value(usize::MAX);
                return;
            }

            let mut path = [0; vfs::MAX_FILE_PATH_LENGTH];
            let n_path = copy_from_user(path_ptr, path.as_mut_ptr(), vfs::MAX_FILE_PATH_LENGTH);
            if n_path == 0 {
                tf.set_syscall_return_value(usize::MAX);
                return;
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
                *pid_ptr = match task::spawn(&path[..n_path], &[], Some(&this_ctx.current_task)) {
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
        _ => unreachable!(),
    }
}

/// Copies from user buffer until it sees NULL.
pub fn copy_from_user(user_ptr: *const u8, dest_ptr: *mut u8, max: usize) -> usize {
    for i in 0..max {
        unsafe {
            let b = *user_ptr.add(i);
            if b == 0 {
                return i;
            }
            *dest_ptr.add(i) = b;
        }
    }

    0
}
