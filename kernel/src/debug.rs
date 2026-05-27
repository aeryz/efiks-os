use ksync::SpinLock;

static CONSOLE_LOCK: SpinLock<()> = SpinLock::new(());

pub fn printk<T: AsRef<[u8]>>(b: T) {
    // TODO(aeryz): THIS IS GONNA 100% DEADLOCK UNLESS WE MAKE SURE THE INTERRUPTS
    // ARE DISABLED. maybe a interrupt disabling spinlock where calling `lock`
    // always disables interrupts first?
    let _console_guard = CONSOLE_LOCK.lock();
    b.as_ref()
        .into_iter()
        .for_each(|b| riscv::sbi::console_putchar(*b));
}
