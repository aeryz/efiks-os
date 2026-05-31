const efiks = @import("efiks");

export fn _start() noreturn {
    while (true) {
        _ = efiks.write("hello world");
        efiks.syscall_sleep_ms(1200);
    }
}
