const efiks = @import("efiks");

export fn _start() callconv(.naked) noreturn {
    asm volatile (
        \\  mv a0, sp
        \\  tail __efiks_start
        ::: .{ .memory = true });
}

export fn __efiks_start(sp: usize) callconv(.c) noreturn {
    const argc_ptr: *const usize = @ptrFromInt(sp);
    const argc = argc_ptr.*;

    const argv: [*]const ?[*:0]const u8 = @ptrFromInt(sp + @sizeOf(usize));

    const exit_code = main(argc, argv);

    efiks.syscall_exit(exit_code);
}

fn main(_: usize, argv: [*]const ?[*:0]const u8) i32 {
    var i: usize = 0;
    while (argv[i]) |arg| : (i += 1) {
        _ = efiks.syscall_write(arg, strlen(arg));
    }

    _ = efiks.write("hello world from the spawned program\n");
    efiks.syscall_sleep_ms(2000);
    efiks.syscall_exit(1);

    while (true) {}
}

fn strlen(s: [*:0]const u8) usize {
    var len: usize = 0;
    while (s[len] != 0) : (len += 1) {}
    return len;
}
