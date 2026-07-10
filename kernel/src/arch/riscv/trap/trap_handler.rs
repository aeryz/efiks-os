use crate::{
    arch::{
        mmu::PteFlags,
        plic::{self, plic_claim, plic_complete},
        riscv::trap::trap_frame::{TrapCause, TrapFrame},
    },
    driver,
    mm::VirtAddr,
    sched, syscall, task,
};

#[unsafe(no_mangle)]
extern "C" fn trap_handler(trap_frame: &mut TrapFrame) {
    // https://docs.riscv.org/reference/isa/priv/supervisor.html#scause
    match trap_frame.get_cause() {
        // TODO(aeryz): right now, we don't have ISA-independent drivers. Keeping this as is
        // right now but this is no good.
        TrapCause::ExternalIrq => {
            log::trace!("hit external irq");
            let hart_id = sched::load_core_ctx().core_id;

            let interrupt_id = plic_claim(hart_id);
            match interrupt_id {
                0 => {}
                plic::UART0_IRQ => {
                    let mut read_anything = false;
                    while let Some(_) = driver::uart::read_char_into_buf() {
                        log::trace!("read somethgin");
                        read_anything = true;
                    }
                    if read_anything {
                        log::trace!("read a bunch of things");
                        sched::on_external_irq(interrupt_id);
                    }
                    log::trace!("uart interrupt happened");
                }
                irq_id => {
                    log::warn!("unhandled irq {irq_id}");
                }
            }
            if interrupt_id != 0 {
                plic_complete(hart_id, interrupt_id);
            }
        }
        TrapCause::TimerInterrupt => {
            sched::on_timer_interrupt();
        }
        TrapCause::Syscall => {
            // This is a syscall, so we move the return program counter to just after the
            // `ecall`
            trap_frame.sepc += 4;
            syscall::dispatch_syscall(trap_frame);
        }
        TrapCause::LoadPageFault => {
            let faulting_address = VirtAddr::new(riscv::registers::Stval::read().raw());

            task::on_page_fault(
                &sched::load_core_ctx().current_task,
                faulting_address,
                PteFlags::R,
            );
        }
        TrapCause::StorePageFault => {
            let faulting_address = VirtAddr::new(riscv::registers::Stval::read().raw());

            task::on_page_fault(
                &sched::load_core_ctx().current_task,
                faulting_address,
                PteFlags::W,
            );
        }
        TrapCause::Unknown(trap) => {
            let fp = trap_frame.s0;

            panic!(
                "unknown trap: {trap} (fp: 0x{:x}, sepc: 0x{:x}, stval: 0x{:x}, stvec: 0x{:x})",
                fp,
                riscv::registers::Sepc::read().raw(),
                riscv::registers::Stval::read().raw(),
                riscv::registers::Stvec::read().raw()
            );
        }
    }
}
