use crate::{
    Arch,
    arch::{
        Architecture,
        plic::{self, plic_claim},
        riscv::trap::trap_frame::{TrapCause, TrapFrame},
    },
    percpu::PerCoreContext,
    sched, syscall, task,
};

#[unsafe(no_mangle)]
extern "C" fn trap_handler(trap_frame: &mut TrapFrame) {
    // https://docs.riscv.org/reference/isa/priv/supervisor.html#scause
    match trap_frame.get_cause() {
        // TODO(aeryz): right now, we don't have ISA-independent drivers. Keeping this as is
        // right now but this is no good.
        TrapCause::ExternalIrq => {
            let hart_id = unsafe {
                Arch::load_this_cpu_ctx::<PerCoreContext>()
                    .as_mut()
                    .expect("expected a valid reference to the per-CPU context")
                    .core_id
            };

            let interrupt_id = plic_claim(hart_id);
            match interrupt_id {
                plic::UART0_IRQ => {
                    log::trace!("uart interrupt happened");
                }
                irq_id => {
                    log::warn!("unhandled irq {irq_id}");
                }
            }
        }
        TrapCause::TimerInterrupt => {
            sched::timer_interrupt();
        }
        TrapCause::Syscall => {
            // This is a syscall, so we move the return program counter to just after the
            // `ecall`
            trap_frame.sepc += 4;
            syscall::dispatch_syscall(trap_frame);
        }
        TrapCause::Unknown(trap) => {
            panic!(
                "unknown trap: {trap} (sepc: 0x{:x}, stvec: 0x{:x})",
                riscv::registers::Sepc::read().raw(),
                riscv::registers::Stvec::read().raw()
            );
        }
    }
}
