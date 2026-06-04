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
        Syscall::Spawn => {}
        _ => unreachable!(),
    }
}
