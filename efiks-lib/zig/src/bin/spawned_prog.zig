const efiks = @import("efiks");

export fn _start() noreturn {
    // while (true) {
    _ = efiks.write("hello world from the spawned program\n");
    efiks.syscall_sleep_ms(2000);
    // }
    efiks.syscall_exit(1);

    while (true) {}
}
