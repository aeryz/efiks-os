use core::ptr;

use crate::{
    Arch,
    arch::{Architecture, TrapFrame, TrapFrameOf},
    percpu, sched, task,
};

#[repr(usize)]
pub enum Syscall {
    Write = 1,
    Read,
    SleepMs,
    Shutdown,
    Exit,
    Spawn,
    End,
}

// TODO(aeryz): We don't want to implement the syscalls here. But they should
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
            let buf = tf.get_arg::<1>() as *const u8;
            let count = tf.get_arg::<2>();

            let this_ctx = unsafe {
                Arch::load_this_cpu_ctx::<percpu::PerCoreContext>()
                    .as_ref()
                    .unwrap()
            };

            let current_task = unsafe { this_ctx.currently_running_task.as_ref() };
            let file = {
                let file_table = current_task.file_table.lock();
                file_table.get_file(fd)
            };

            let Some(file) = file else {
                tf.set_syscall_return_value(0);
                return;
            };

            let count = file
                .lock()
                .write(unsafe { core::slice::from_raw_parts(buf, count) })
                .unwrap_or(usize::MAX);

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

            let current_task = unsafe { this_ctx.currently_running_task.as_ref() };
            let file = {
                let file_table = current_task.file_table.lock();
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
        Syscall::SleepMs => {
            let time_ms = tf.get_arg::<0>();
            sched::sleep_current_task(time_ms);
        }
        Syscall::Spawn => {
            let pid_ptr = tf.get_arg::<0>() as *mut task::Pid;
            let path_ptr = tf.get_arg::<1>() as *const u8;

            if pid_ptr == ptr::null_mut() || path_ptr == ptr::null() {
                tf.set_syscall_return_value(usize::MAX);
                return;
            }

            let path = unsafe {
                let mut count = 0;
                loop {
                    if count >= vfs::MAX_FILE_PATH_LENGTH {
                        tf.set_syscall_return_value(usize::MAX);
                        return;
                    }

                    if *(path_ptr.offset(count as isize)) == 0 {
                        break core::slice::from_raw_parts(path_ptr, count);
                    }

                    count += 1;
                }
            };

            let this_ctx = unsafe {
                Arch::load_this_cpu_ctx::<percpu::PerCoreContext>()
                    .as_mut()
                    .unwrap()
            };

            unsafe {
                *pid_ptr = match task::spawn(path, Some(this_ctx.currently_running_task)) {
                    Ok(pid) => pid,
                    Err(_) => {
                        tf.set_syscall_return_value(usize::MAX);
                        return;
                    }
                };
            }

            unsafe {
                this_ctx.currently_running_task.as_mut().state = task::TaskState::Blocked;
            }

            tf.set_syscall_return_value(0);

            sched::schedule();
        }
        _ => unreachable!(),
    }
}
