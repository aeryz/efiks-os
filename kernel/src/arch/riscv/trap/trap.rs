use crate::arch::trap::trap_frame::TrapFrame;

unsafe extern "C" {
    pub fn trap_entry();
    pub fn trap_resume();
}

core::arch::global_asm!(
    include_str!("./trap.S"),
    TRAPFRAME_SIZE = const size_of::<TrapFrame>(),
    SAVED_SP_FROM_TOP = const (size_of::<TrapFrame>() - 1 * 8),
    SAVED_TP_FROM_TOP = const (size_of::<TrapFrame>() - 3 * 8),
    SAVED_T0_FROM_TOP = const (size_of::<TrapFrame>() - 4 * 8),
    SAVED_T1_FROM_TOP = const (size_of::<TrapFrame>() - 5 * 8),
    SAVED_SSTATUS_FROM_TOP = const (size_of::<TrapFrame>() - 33 * 8),
    SSTATUS_SPP_MASK = const riscv::registers::Sstatus::SPP_MASK,
);
