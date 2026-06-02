use ksync::SpinLock;

static CONSOLE_LOCK: SpinLock<()> = SpinLock::new(());

pub fn printk<T: AsRef<[u8]>>(b: T) {
    let sstatus = riscv::registers::Sstatus::read().raw();
    riscv::registers::Sstatus::new(sstatus)
        .disable_supervisor_interrupts()
        .write();

    let _console_guard = CONSOLE_LOCK.lock();
    b.as_ref()
        .into_iter()
        .for_each(|b| riscv::sbi::console_putchar(*b));

    drop(_console_guard);
    riscv::registers::Sstatus::new(sstatus).write();
}
