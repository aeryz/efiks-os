#![no_std]
#![no_main]
#![allow(static_mut_refs)]
#![feature(cold_path)]

#[cfg(feature = "riscv-sbi")]
pub type Arch = arch::Riscv;

extern crate alloc;

mod arch;
pub mod console;
mod debug;
mod driver;
pub mod error;
pub mod exec;
mod helper;
mod mm;
mod percpu;
mod sched;
mod serial_log;
mod syscall;
mod task;
mod vfs;

use alloc::{collections::vec_deque::VecDeque, sync::Arc, vec::Vec};
pub use debug::*;
use ksync::SpinLock;

use crate::{
    arch::Architecture,
    driver::{
        uart,
        virtio::{self, block::VirtioBlkDriver},
    },
    mm::VirtAddr,
    percpu::PerCoreContext,
};

core::arch::global_asm!(include_str!("start.s"));

#[unsafe(no_mangle)]
extern "C" fn kmain(hartid: usize, dtb_address: usize) -> ! {
    mm::early_init();

    serial_log::init();
    log::info!("Kernel starts with hart_id: {hartid}, dtb: 0x{dtb_address:x}",);

    let blk_device_base = virtio::find_virtio_blk().expect("virtio must exist");
    log::info!("Found VirtIO device at address: {blk_device_base:x}");

    virtio::block::init(blk_device_base).expect("driver must be initialized");
    log::info!("VirtIO driver is initialized");

    vfs::mount::<VirtioBlkDriver>(b"/", vfs::SupportedFs::Vsfs)
        .expect("The filesystem should be able to be mounted at root");

    let mut core_ctxs = Vec::new();

    setup_core(hartid, &mut core_ctxs);
    #[cfg(feature = "multi-core")]
    {
        setup_core(1, &mut core_ctxs);
        setup_core(2, &mut core_ctxs);
    }

    percpu::set_core_ctxs(core_ctxs);

    task::spawn(b"/foo/shell", &[], None).unwrap();

    #[cfg(feature = "multi-core")]
    {
        Arch::boot_core(1);
        Arch::boot_core(2);
    }
    core_boot_entry(0);
}

fn setup_core(core_id: usize, core_ctxs: &mut Vec<percpu::PerCoreContext>) {
    let idle_task =
        task::create_kernel_task(VirtAddr::new(idle_task_main as *const () as usize)).unwrap();

    let reaper_task =
        task::create_kernel_task(VirtAddr::new(sched::reaper_task_main as *const () as usize))
            .unwrap();

    core_ctxs.push(percpu::PerCoreContext {
        core_id,
        scheduler: SpinLock::new(sched::init_per_core_scheduler(Arc::clone(&reaper_task))),
        current_task: Arc::clone(&idle_task),
        idle_task,
        reaper_task: percpu::ReaperTaskCtx {
            task: reaper_task,
            cleanup_queue: SpinLock::new(VecDeque::new()),
        },
    });
}

#[unsafe(no_mangle)]
extern "C" fn core_boot_entry(core: usize) -> ! {
    Arch::init_trap_handler();
    log::trace!("trap handler initiated");

    let core_ctx = percpu::get_core(core);
    unsafe {
        (*core_ctx.idle_task.thread_info.per_cpu_ctx.get()) =
            core_ctx as *const _ as *mut PerCoreContext;
        (*core_ctx.reaper_task.task.thread_info.per_cpu_ctx.get()) =
            core_ctx as *const _ as *mut PerCoreContext;
    }
    Arch::set_per_cpu_ctx_ptr(VirtAddr::new(
        core_ctx.idle_task.as_ref() as *const _ as usize
    ));

    Arch::init_uart(core_ctx.core_id);
    log::trace!("uart initiated for hart {}", core_ctx.core_id);

    uart::enable_interrupts();
    log::trace!("uart interrupts enabled");

    Arch::setup_unpriviledged_mode();

    let time = Arch::read_current_time();
    Arch::set_timer(time + Arch::nanos_to_ticks(32 * 1_000_000));

    Arch::enable_interrupts();

    sched::schedule();

    idle_task_main();
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    log::error!("KERNEL PANIC: {}", info.message());
    if let Some(loc) = info.location() {
        log::error!("-> File: {} at line: {}", loc.file(), loc.line());
    }

    loop {
        Arch::halt();
    }
}

#[unsafe(no_mangle)]
#[inline(never)]
extern "C" fn idle_task_main() -> ! {
    loop {
        riscv::registers::Sstatus::read()
            .enable_supervisor_interrupts()
            .write();
        unsafe {
            core::arch::asm!("wfi");
        }
    }
}
