use crate::arch::{self, Riscv, VirtualAddressOf};

#[repr(C)]
#[derive(Clone, Default)]
pub struct TrapFrame {
    pub(super) ra: usize,
    pub(super) sp: usize,
    pub(super) gp: usize,
    pub(super) tp: usize,
    pub(super) t0: usize,
    pub(super) t1: usize,
    pub(super) t2: usize,
    pub(super) s0: usize,
    pub(super) s1: usize,
    pub(super) a0: usize,
    pub(super) a1: usize,
    pub(super) a2: usize,
    pub(super) a3: usize,
    pub(super) a4: usize,
    pub(super) a5: usize,
    pub(super) a6: usize,
    pub(super) a7: usize,
    pub(super) s2: usize,
    pub(super) s3: usize,
    pub(super) s4: usize,
    pub(super) s5: usize,
    pub(super) s6: usize,
    pub(super) s7: usize,
    pub(super) s8: usize,
    pub(super) s9: usize,
    pub(super) s10: usize,
    pub(super) s11: usize,
    pub(super) t3: usize,
    pub(super) t4: usize,
    pub(super) t5: usize,
    pub(super) t6: usize,
    pub(super) sepc: usize,
    pub(super) scause: usize,
    pub(super) sstatus: usize,
}

impl TrapFrame {
    pub(super) fn get_cause(&self) -> TrapCause {
        self.scause.into()
    }
}

pub enum TrapCause {
    Syscall,
    TimerInterrupt,
    ExternalIrq,
    Unknown(usize),
}

impl From<usize> for TrapCause {
    fn from(value: usize) -> Self {
        match value {
            0x8 => TrapCause::Syscall,
            0x8000000000000005 => TrapCause::TimerInterrupt,
            0x8000000000000009 => TrapCause::ExternalIrq,
            _ => TrapCause::Unknown(value),
        }
    }
}

impl arch::TrapFrame<Riscv> for TrapFrame {
    fn initialize(
        instruction_ptr: VirtualAddressOf<Riscv>,
        stack_ptr: VirtualAddressOf<Riscv>,
    ) -> Self {
        Self {
            sepc: instruction_ptr.into(),
            sp: stack_ptr.into(),
            sstatus: riscv::registers::Sstatus::empty()
                .enable_user_mode()
                .set_spie()
                .enable_user_page_access()
                .raw() as usize,
            ..Default::default()
        }
    }

    fn get_arg<const I: usize>(&self) -> usize {
        match I {
            0 => self.a0,
            1 => self.a1,
            2 => self.a2,
            3 => self.a3,
            4 => self.a4,
            5 => self.a5,
            6 => self.a6,
            7 => self.a7,
            _ => panic!("invalid"),
        }
    }

    fn get_syscall(&self) -> usize {
        self.a7
    }

    fn set_syscall_return_value(&mut self, ret: usize) {
        self.a0 = ret;
    }

    fn set_per_core_ctx(&mut self, ptr: usize) {
        self.tp = ptr;
    }
}
