const efiks = @import("efiks");

export fn _start() noreturn {
    while (true) {
        _ = efiks.write("hello world from the spawned program\n");
        efiks.syscall_shutdown();
    }
}
