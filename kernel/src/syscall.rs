use crate::{
    Arch,
    arch::{Architecture, TrapFrame, TrapFrameOf},
    percpu, sched,
};

pub const SYSCALL_WRITE: usize = 1;
pub const SYSCALL_READ: usize = 2;
pub const SYSCALL_SLEEP_MS: usize = 3;
// TODO(aeryz): this is not supposed to be a syscall. It's here for convenience
// only.
pub const SYSCALL_SHUTDOWN: usize = 4;
pub const SYSCALL_EXIT: usize = 5;

// TODO(aeryz): We don't want to implement the syscalls here. But they should
// directly be implemented in their respective subsystem.

#[unsafe(no_mangle)]
#[inline(never)]
pub fn dispatch_syscall(tf: &mut TrapFrameOf<Arch>) {
    let syscall_number = tf.get_syscall();
    match syscall_number {
        SYSCALL_WRITE => {
            let _fd = tf.get_arg::<0>();
            let buf = tf.get_arg::<1>() as *const u8;
            let count = tf.get_arg::<2>();

            let utf8_str =
                unsafe { str::from_utf8_unchecked(core::slice::from_raw_parts(buf, count)) };

            let this_ctx = unsafe {
                Arch::load_this_cpu_ctx::<percpu::PerCoreContext>()
                    .as_ref()
                    .unwrap()
            };

            log::trace!(
                "this syscall is called by {} with buf: {:?}",
                this_ctx.core_id,
                buf
            );
            crate::printk(utf8_str);

            tf.set_syscall_return_value(count);
        }
        SYSCALL_SLEEP_MS => {
            let time_ms = tf.get_arg::<0>();
            sched::sleep_current_task(time_ms);
        }
        _ => unreachable!(),
    }
}
