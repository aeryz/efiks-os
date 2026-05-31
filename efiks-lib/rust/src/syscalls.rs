use core::arch::asm;

pub const SYSCALL_WRITE: usize = 1;
pub const SYSCALL_READ: usize = 2;
pub const SYSCALL_SLEEP_MS: usize = 3;
// TODO(aeryz): this is not supposed to be a syscall. It's here for convenience
// only.
pub const SYSCALL_SHUTDOWN: usize = 4;
pub const SYSCALL_EXIT: usize = 5;

pub fn write(data_ptr: *const u8, len: usize) -> isize {
    let ret: isize;
    unsafe {
        asm!(
            "li a0, 1",
            "ecall",
            in("a7") SYSCALL_WRITE,
            in("a1") data_ptr,
            in("a2") len,
            lateout("a0") ret,
            options(nostack),
        )
    }

    ret
}

pub fn read(buf: *mut u8, count: usize) -> isize {
    let ret: isize;
    unsafe {
        asm!(
            "li a0, 0",
            "ecall",
            in("a7") SYSCALL_READ,
            in("a1") buf,
            in("a2") count,
            lateout("a0") ret,
            options(nostack),
        )
    }

    ret
}

pub fn sleep_ms(ms: usize) {
    unsafe {
        asm!(
            "ecall",
            in("a7") SYSCALL_SLEEP_MS,
            in("a0") ms,
            options(nostack)
        )
    }
}

// TODO: temporary syscall
pub fn shutdown() {
    unsafe {
        asm!(
            "ecall",
            in("a7") SYSCALL_SHUTDOWN,
            options(nostack)
        )
    }
}

pub fn exit(exit_code: i32) {
    unsafe {
        asm!(
            "ecall",
            in("a7") SYSCALL_EXIT,
            in("a0") exit_code,
            options(nostack)
        )
    }
}
