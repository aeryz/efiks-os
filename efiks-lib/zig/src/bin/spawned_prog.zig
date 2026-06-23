const efiks = @import("efiks");
const std = @import("std");

pub const std_options = std.Options{
    .page_size_max = 4096,
};

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

    const exit_code = main(argc, argv) catch -1;

    efiks.syscall_exit(exit_code);

    while (true) {}
}

const brk_allocator: std.mem.Allocator = .{
    .ptr = undefined,
    .vtable = &efiks.BrkAllocator.vtable,
};

fn main(argc: usize, argv: [*]const ?[*:0]const u8) !i32 {
    var arena = std.heap.ArenaAllocator.init(brk_allocator);
    var allocator = arena.allocator();
    const ptr = try allocator.create(usize);
    ptr.* = argc;

    var buf: [64]u8 = undefined;
    const s = try std.fmt.bufPrint(&buf, "Ptr is {} {} {}?", .{ argc, ptr.*, ptr });
    _ = efiks.write(@constCast(s));

    var i: usize = 0;
    while (argv[i]) |arg| : (i += 1) {
        _ = efiks.syscall_write(arg, strlen(arg));
    }

    _ = efiks.write("hello world from the spawned program\n");
    efiks.syscall_sleep_ms(2000);

    return 0;
}

fn strlen(s: [*:0]const u8) usize {
    var len: usize = 0;
    while (s[len] != 0) : (len += 1) {}
    return len;
}
