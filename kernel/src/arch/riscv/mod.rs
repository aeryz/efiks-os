mod boot;
mod context;
pub mod mmu;
pub mod plic;
pub mod trap;

use core::{cell::OnceCell, ptr::NonNull};

use riscv::registers::Satp;

use crate::{
    arch::{
        Architecture, MemoryModel, PhysicalAddressOf, VirtualAddressOf,
        mmu::{PageTable, PhysicalAddress, VirtualAddress},
        trap::{
            trap::{trap_entry, trap_resume},
            trap_frame::TrapFrame,
        },
    },
    mm::{self, KernelVirtAddr, VirtAddr},
};

use context::Context;

pub struct Riscv;

impl Architecture for Riscv {
    const CPU_HERTZ: usize = 10_000_000;

    type MemoryModel = Self;

    type TrapFrame = TrapFrame;

    type Context = Context;

    fn bump_sp(sp: usize) {
        riscv::add_to_sp(sp);
    }

    fn load_this_cpu_ctx<T>() -> *mut T {
        riscv::read_tp() as *mut T
    }

    fn read_current_time() -> usize {
        riscv::registers::Time::read().raw()
    }

    fn init_trap_handler() {
        riscv::registers::Stvec::new(trap_entry as *const () as usize).write();
    }

    fn enable_interrupts() {
        riscv::registers::Sstatus::read()
            .enable_supervisor_interrupts()
            .write();

        riscv::registers::Sie::empty()
            .enable_external_interrupts()
            .enable_timer_interrupt()
            .write();
    }

    fn init_uart(core_id: usize) {
        plic::plic_init_uart(core_id);
    }

    fn switch_to(from: *mut Self::Context, to: *const Self::Context) {
        unsafe { context::swtch(from, to) };
    }

    fn switch_to_user(
        from: *mut Self::Context,
        to: *const Self::Context,
        root_pt: PhysicalAddressOf<Self>,
    ) {
        unsafe { context::swtch_to_user(from, to, mmu::pa_to_satp(root_pt)) };
    }

    fn set_per_cpu_ctx_ptr(ptr: VirtualAddressOf<Self>) {
        unsafe {
            core::arch::asm!(
                "mv tp, {}",
                in(reg) ptr.raw()
            );
        }
    }

    fn trap_resume_ptr() -> KernelVirtAddr {
        // TODO(aeryz): we want to put this in a static to not gamble on compiler
        // optimization
        KernelVirtAddr::new(VirtAddr::new(trap_resume as *const () as usize))
            .expect("trap resume is at a valid kernel address")
    }

    fn setup_unpriviledged_mode() {
        riscv::registers::Sstatus::read()
            .enable_user_mode()
            .enable_user_page_access()
            .write();
    }

    fn set_kernel_sp(sp: Option<VirtualAddressOf<Self>>) {
        riscv::registers::Sscratch::new(match sp {
            None => 0,
            Some(sp) => sp.raw(),
        })
        .write();
    }

    fn set_timer(time_val: usize) {
        riscv::sbi::set_timer(time_val);
    }

    fn flush_tlb() {
        unsafe {
            core::arch::asm!("sfence.vma zero, zero");
        }
    }

    fn halt() {
        riscv::sbi::shutdown();
    }

    fn boot_core(core_id: usize) {
        let mut sp = mm::alloc_frame().unwrap().raw() + 0xff0;

        sp = sp - size_of::<usize>();

        unsafe {
            let sp_kernel_view = mm::virt_to_phys(sp);
            *(sp_kernel_view as *mut usize) = <Self as MemoryModel>::get_root_page_table();
        }

        let ret = riscv::sbi::hart_start(
            core_id,
            core_entry_trampoline as *const () as usize
                - (mm::KERNEL_IMAGE_START_VA.raw() - mm::KERNEL_IMAGE_START_PA.raw()),
            sp,
        );

        if ret.error == 0 {
            log::info!("core {core_id} started successfully");
        } else {
            log::error!("core {core_id} start failure");
            panic!();
        }
    }
}

// TODO(aeryz): This contains arch specific code, move it to `arch/boot`
#[unsafe(naked)]
#[allow(unused)]
extern "C" fn core_entry_trampoline() -> ! {
    core::arch::naked_asm!(
        r#"
        mv sp, a1
        ld a2, 0(sp)

        csrw satp, a2
        sfence.vma

        li t0, {kernel_offset}
        la t1, core_boot_entry
        add t0, t0, t1

        li t1, {kernel_direct_mapping_base}
        add sp, sp, t1

        jr t0
        "#,
        kernel_offset = const (mm::KERNEL_IMAGE_START_VA.raw() - mm::KERNEL_IMAGE_START_PA.raw()),
        kernel_direct_mapping_base = const (mm::KERNEL_DIRECT_MAPPING_BASE.raw()),
    )
}

impl MemoryModel for Riscv {
    type PhysicalAddress = PhysicalAddress;

    type VirtualAddress = VirtualAddress;

    fn set_root_page_table_pa(pa: Self::PhysicalAddress) {
        mmu::set_root_page_table_pa(pa);
    }

    fn get_root_page_table() -> usize {
        Satp::read().raw() as usize
    }

    fn set_root_page_table(val: usize) {
        mmu::set_root_page_table(val);
    }
}
