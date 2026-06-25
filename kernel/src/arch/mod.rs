#![allow(unused)]

#[cfg(feature = "riscv-sbi")]
mod riscv;

use core::ptr::NonNull;

#[cfg(feature = "riscv-sbi")]
pub use riscv::*;

use crate::mm::{KernelVirtAddr, VirtAddr};

/// Defines all the architecture-dependent functionality.
pub trait Architecture {
    const CPU_HERTZ: usize;

    type TrapFrame: TrapFrame;

    type MemoryModel: MemoryModel;

    type Context: Context;

    #[inline(always)]
    fn bump_sp(sp: usize);

    /// Loads the pointer to the current CPU context.
    ///
    /// SAFETY:
    /// - It's totally kernel's responsibility to properly set the CPU context.
    #[inline(always)]
    fn load_this_cpu_ctx<T>() -> *mut T;

    /// Reads the current time
    fn read_current_time() -> usize;

    /// Sets the trap handler
    fn init_trap_handler();

    fn enable_interrupts();

    // TODO(aeryz): We probably don't want this like this but for now, we have this
    fn init_uart(core_id: usize);

    fn switch_to(from: *mut Self::Context, to: *const Self::Context);

    fn switch_to_user(
        from: *mut Self::Context,
        to: *const Self::Context,
        root_pt: PhysicalAddressOf<Self>,
    );

    fn set_per_cpu_ctx_ptr(ptr: VirtualAddressOf<Self>);

    /// The address where a first time spawned process jump to,
    /// should be right after calling the trap handler in the trap entry
    fn trap_resume_ptr() -> KernelVirtAddr;

    fn setup_unpriviledged_mode();

    fn set_kernel_sp(sp: Option<VirtualAddressOf<Self>>);

    fn set_timer(time_val: usize);

    fn flush_tlb();

    fn nanos_to_ticks(nanos: usize) -> usize {
        nanos * Self::CPU_HERTZ / 1_000_000_000
    }

    fn halt();

    /// Boots the core `core_id` and jumps to `core_boot_entry` by only giving
    /// the `core_id` as the parameter to it.
    fn boot_core(core_id: usize);
}

pub type VirtualAddressOf<Arch> =
    <<Arch as Architecture>::MemoryModel as MemoryModel>::VirtualAddress;
pub type PhysicalAddressOf<Arch> =
    <<Arch as Architecture>::MemoryModel as MemoryModel>::PhysicalAddress;
pub type TrapFrameOf<Arch> = <Arch as Architecture>::TrapFrame;
pub type ContextOf<Arch> = <Arch as Architecture>::Context;

pub trait MemoryModel {
    type PhysicalAddress: Into<usize>;

    type VirtualAddress: Into<usize>;

    fn set_root_page_table_pa(pa: Self::PhysicalAddress);

    fn set_root_page_table(val: usize);

    fn get_root_page_table() -> usize;
}

pub trait TrapFrame {
    fn initialize(instruction_ptr: VirtAddr, stack_ptr: VirtAddr) -> Self;

    fn get_syscall(&self) -> usize;

    fn set_syscall_return_value(&mut self, ret: usize);

    fn get_arg<const I: usize>(&self) -> usize;

    fn set_per_core_ctx(&mut self, ptr: usize);
}

pub trait Context {
    fn initialize(entry: VirtAddr, kernel_sp: VirtAddr) -> Self;
}
