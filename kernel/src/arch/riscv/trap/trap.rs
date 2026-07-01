use crate::arch::trap::trap_frame::TrapFrame;

unsafe extern "C" {
    pub fn trap_entry();
    pub fn trap_resume();
}

core::arch::global_asm!(
r#"
    .section .text.trap
    .globl trap_entry
    .globl trap_resume
    .align 2
trap_entry:

    // Swap the TLS and ThreadInfo
    csrrw tp, sscratch, tp

    // On kernel threads, sscratch is 0, so we just load it back in
    bnez tp, save_user_stack
    csrr tp, sscratch
    j save_kernel_stack

// On user threads, user sp needs to be stored and kernel sp needs to be loaded
save_user_stack:
    sd sp, 0*8(tp)
    ld sp, 1*8(tp)

    // Allocate the stack pointer to fit the trapframe
    addi sp, sp, -{TRAPFRAME_SIZE}

    // Now we can start saving the registers into the trap frame.
    // Otherwise, there is no guarantee that our registers will not be
    // altered with. (had a painful experience with this)

    sd ra,  0*8(sp)
    ld ra, 0*8(tp)
    sd ra,  1*8(sp)
    j save_trapframe

save_kernel_stack:
    // Allocate the stack pointer to fit the trapframe
    addi sp, sp, -{TRAPFRAME_SIZE}
    sd ra,  0*8(sp)
    addi ra, sp, {TRAPFRAME_SIZE}
    sd ra,  1*8(sp)

save_trapframe:
    // read the user tp
    csrr ra, sscratch
    sd ra,  3*8(sp)
    // then restore the ra
    ld ra,  0*8(sp)
    csrw sscratch, zero
    sd gp,  2*8(sp)
    sd t0,  4*8(sp)
    sd t1,  5*8(sp)
    sd t2,  6*8(sp)
    sd s0,  7*8(sp)
    sd s1,  8*8(sp)
    sd a0,  9*8(sp)
    sd a1,  10*8(sp)
    sd a2,  11*8(sp)
    sd a3,  12*8(sp)
    sd a4,  13*8(sp)
    sd a5,  14*8(sp)
    sd a6,  15*8(sp)
    sd a7,  16*8(sp)
    sd s2,  17*8(sp)
    sd s3,  18*8(sp)
    sd s4,  19*8(sp)
    sd s5,  20*8(sp)
    sd s6,  21*8(sp)
    sd s7,  22*8(sp)
    sd s8,  23*8(sp)
    sd s9,  24*8(sp)
    sd s10, 25*8(sp)
    sd s11, 26*8(sp)
    sd t3,  27*8(sp)
    sd t4,  28*8(sp)
    sd t5,  29*8(sp)
    sd t6,  30*8(sp)

    csrr t0, sepc
    sd t0, 31*8(sp)

    csrr t0, scause
    sd t0, 32*8(sp)

    csrr t0, sstatus
    sd t0, 33*8(sp)
   
    // Move the trap frame (sitting at sp) as the first param
    mv a0, sp
    call trap_handler

trap_resume:
    ld t0, 31*8(sp)
    csrw sepc, t0

    ld t0, 33*8(sp)
    csrw sstatus, t0

    ld ra,  0*8(sp)
    ld gp,  2*8(sp)
    // t0 and t1 are restored at the end because trap return needs scratch
    // registers to decide whether this was a user or kernel trap.
    ld t2,  6*8(sp)
    ld s0,  7*8(sp)
    ld s1,  8*8(sp)
    ld a0,  9*8(sp)
    ld a1,  10*8(sp)
    ld a2,  11*8(sp)
    ld a3,  12*8(sp)
    ld a4,  13*8(sp)
    ld a5,  14*8(sp)
    ld a6,  15*8(sp)
    ld a7,  16*8(sp)
    ld s2,  17*8(sp)
    ld s3,  18*8(sp)
    ld s4,  19*8(sp)
    ld s5,  20*8(sp)
    ld s6,  21*8(sp)
    ld s7,  22*8(sp)
    ld s8,  23*8(sp)
    ld s9,  24*8(sp)
    ld s10, 25*8(sp)
    ld s11, 26*8(sp)
    ld t3,  27*8(sp)
    ld t4,  28*8(sp)
    ld t5,  29*8(sp)
    ld t6,  30*8(sp)

    // Restore the stack pointer to the top of the trap frame.
    addi sp, sp, {TRAPFRAME_SIZE}

    // If saved sstatus.SPP is set, the trap came from supervisor mode.
    // Kernel traps use sscratch = 0; user traps use sscratch = thread_info ptr.
    ld t0, -{SAVED_SSTATUS_FROM_TOP}(sp)
    li t1, {SSTATUS_SPP_MASK}
    and t0, t0, t1
    bnez t0, ret_kernel

ret_userspace:
    csrw sscratch, tp
    ld t0, -{SAVED_T0_FROM_TOP}(sp)
    ld t1, -{SAVED_T1_FROM_TOP}(sp)
    ld tp, -{SAVED_TP_FROM_TOP}(sp)
    ld sp, -{SAVED_SP_FROM_TOP}(sp)
    sret

ret_kernel:
    csrw sscratch, zero
    ld t0, -{SAVED_T0_FROM_TOP}(sp)
    ld t1, -{SAVED_T1_FROM_TOP}(sp)
    ld sp, -{SAVED_SP_FROM_TOP}(sp)
    sret
        "#,
TRAPFRAME_SIZE = const size_of::<TrapFrame>(),
SAVED_SP_FROM_TOP = const (size_of::<TrapFrame>() - 1 * 8),
SAVED_TP_FROM_TOP = const (size_of::<TrapFrame>() - 3 * 8),
SAVED_T0_FROM_TOP = const (size_of::<TrapFrame>() - 4 * 8),
SAVED_T1_FROM_TOP = const (size_of::<TrapFrame>() - 5 * 8),
SAVED_SSTATUS_FROM_TOP = const (size_of::<TrapFrame>() - 33 * 8),
SSTATUS_SPP_MASK = const riscv::registers::Sstatus::SPP_MASK,
);
