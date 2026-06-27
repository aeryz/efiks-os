use alloc::vec::Vec;

use crate::{
    Arch,
    arch::{Architecture, TrapFrame, TrapFrameOf},
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
            let user_buf = UserBuf::new(tf.get_arg::<1>()).unwrap();
            let count = tf.get_arg::<2>();

            // TODO(aeryz): we want to set error when buf_ptr == NULL?
            if count == 0 {
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

            unsafe { user_buf.copy_from_user(&mut buf) };

            let count = file.lock().write(&buf[0..count]).unwrap_or(usize::MAX);

            tf.set_syscall_return_value(count);
        }
        Syscall::Read => {
            let fd = tf.get_arg::<0>();
            let mut user_buf = UserBufMut::new(tf.get_arg::<1>()).unwrap();
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
            unsafe {
                user_buf.copy_into_user(&buf[0..count]);
            }

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
