#![no_std]
#![no_main]

#[unsafe(no_mangle)]
extern "C" fn _start() {
    loop {
        let _ = efiks_lib::write("[task-1] writing babeee\n");
        efiks_lib::syscalls::sleep_ms(1200);
    }
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {
        core::hint::spin_loop();
    }
}
