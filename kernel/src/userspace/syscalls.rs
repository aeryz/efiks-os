use core::arch::asm;

use crate::syscall::Syscall;

pub fn write(data_ptr: *const u8, len: usize) -> isize {
    let ret: isize;
    unsafe {
        asm!(
            "li a0, 1",
            "ecall",
            in("a7") Syscall::Write as usize,
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
            in("a7") Syscall::Read as usize,
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
            in("a7") Syscall::SleepMs as usize,
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
            in("a7") Syscall::Shutdown as usize,
            options(nostack)
        )
    }
}

pub fn exit(exit_code: i32) {
    unsafe {
        asm!(
            "ecall",
            in("a7") Syscall::Exit as usize,
            in("a0") exit_code,
            options(nostack)
        )
    }
}
