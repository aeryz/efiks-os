use core::arch::global_asm;

use crate::{
    arch::{self, Riscv},
    mm::VirtAddr,
    task::Task,
};

const CONTEXT_OFFSET: usize = core::mem::offset_of!(Task, context);

#[unsafe(naked)]
#[unsafe(no_mangle)]
pub extern "C" fn swtch_to_user(from: *const Task, to: *const Task, satp: usize) {
    core::arch::naked_asm!(
        r#"
        csrw satp, a2
        sfence.vma zero, zero
        j swtch
        "#,
    );
}

#[unsafe(naked)]
#[unsafe(no_mangle)]
pub extern "C" fn swtch(from: *const Task, to: *const Task) {
    core::arch::naked_asm!(
        r#"
        addi a0, a0, {context_offset}
        sd ra,   0*8(a0)
        sd sp,   1*8(a0)
        sd s0,   2*8(a0)
        sd s1,   3*8(a0)
        sd s2,   4*8(a0)
        sd s3,   5*8(a0)
        sd s4,   6*8(a0)
        sd s5,   7*8(a0)
        sd s6,   8*8(a0)
        sd s7,   9*8(a0)
        sd s8,  10*8(a0)
        sd s9,  11*8(a0)
        sd s10, 12*8(a0)
        sd s11, 13*8(a0)

        mv tp, a1
        addi a1, a1, {context_offset}
        ld ra,   0*8(a1)
        ld sp,   1*8(a1)
        ld s0,   2*8(a1)
        ld s1,   3*8(a1)
        ld s2,   4*8(a1)
        ld s3,   5*8(a1)
        ld s4,   6*8(a1)
        ld s5,   7*8(a1)
        ld s6,   8*8(a1)
        ld s7,   9*8(a1)
        ld s8,  10*8(a1)
        ld s9,  11*8(a1)
        ld s10, 12*8(a1)
        ld s11, 13*8(a1)

        ret
    "#,
        context_offset = const core::mem::offset_of!(Task, context),
    );
}

#[derive(Clone, Default)]
#[repr(C)]
pub struct Context {
    pub ra: u64,
    pub sp: u64,
    pub s0: u64,
    pub s1: u64,
    pub s2: u64,
    pub s3: u64,
    pub s4: u64,
    pub s5: u64,
    pub s6: u64,
    pub s7: u64,
    pub s8: u64,
    pub s9: u64,
    pub s10: u64,
    pub s11: u64,
}

impl arch::Context for Context {
    fn initialize(entry: VirtAddr, kernel_sp: VirtAddr) -> Self {
        Self {
            ra: entry.raw() as u64,
            sp: kernel_sp.raw() as u64,
            ..Default::default()
        }
    }
}
