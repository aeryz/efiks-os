use core::mem::offset_of;

use crate::arch::trap::trap_frame::TrapFrame;

unsafe extern "C" {
    pub fn trap_entry();
    pub fn trap_resume();
}

core::arch::global_asm!(
    include_str!("./trap.S"),
    TRAPFRAME_SIZE = const size_of::<TrapFrame>(),
    SSTATUS_SPP_MASK = const riscv::registers::Sstatus::SPP_MASK,
    TF_RA = const offset_of!(TrapFrame, ra),
    TF_SP = const offset_of!(TrapFrame, sp),
    TF_GP = const offset_of!(TrapFrame, gp),
    TF_TP = const offset_of!(TrapFrame, tp),
    TF_T0 = const offset_of!(TrapFrame, t0),
    TF_T1 = const offset_of!(TrapFrame, t1),
    TF_T2 = const offset_of!(TrapFrame, t2),
    TF_S0 = const offset_of!(TrapFrame, s0),
    TF_S1 = const offset_of!(TrapFrame, s1),
    TF_A0 = const offset_of!(TrapFrame, a0),
    TF_A1 = const offset_of!(TrapFrame, a1),
    TF_A2 = const offset_of!(TrapFrame, a2),
    TF_A3 = const offset_of!(TrapFrame, a3),
    TF_A4 = const offset_of!(TrapFrame, a4),
    TF_A5 = const offset_of!(TrapFrame, a5),
    TF_A6 = const offset_of!(TrapFrame, a6),
    TF_A7 = const offset_of!(TrapFrame, a7),
    TF_S2 = const offset_of!(TrapFrame, s2),
    TF_S3 = const offset_of!(TrapFrame, s3),
    TF_S4 = const offset_of!(TrapFrame, s4),
    TF_S5 = const offset_of!(TrapFrame, s5),
    TF_S6 = const offset_of!(TrapFrame, s6),
    TF_S7 = const offset_of!(TrapFrame, s7),
    TF_S8 = const offset_of!(TrapFrame, s8),
    TF_S9 = const offset_of!(TrapFrame, s9),
    TF_S10 = const offset_of!(TrapFrame, s10),
    TF_S11 = const offset_of!(TrapFrame, s11),
    TF_T3 = const offset_of!(TrapFrame, t3),
    TF_T4 = const offset_of!(TrapFrame, t4),
    TF_T5 = const offset_of!(TrapFrame, t5),
    TF_T6 = const offset_of!(TrapFrame, t6),
    TF_SEPC = const offset_of!(TrapFrame, sepc),
    TF_SCAUSE = const offset_of!(TrapFrame, scause),
    TF_SSTATUS = const offset_of!(TrapFrame, sstatus),
);
