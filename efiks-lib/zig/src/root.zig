pub const BrkAllocator = @import("brk_allocator.zig");

const Syscall = enum(usize) { write = 1, read, sleep_ms, shutdown, exit, spawn, wait };

pub inline fn write(buf: []const u8) isize {
    return syscall_write(buf.ptr, buf.len);
}

pub inline fn read(buf: []u8) isize {
    return syscall_read(buf.ptr, buf.len);
}

pub fn syscall_write(data_ptr: [*]const u8, len: usize) isize {
    const ret = asm volatile ("ecall"
        : [ret] "={x10}" (-> usize),
        : [number] "{x17}" (Syscall.write),
          [fd] "{x10}" (@as(usize, 1)),
          [data_ptr] "{x11}" (@intFromPtr(data_ptr)),
          [len] "{x12}" (len),
        : .{ .memory = true });

    return @bitCast(ret);
}

pub fn syscall_read(buf: [*]u8, count: usize) isize {
    const ret = asm volatile ("ecall"
        : [ret] "={x10}" (-> usize),
        : [number] "{x17}" (Syscall.read),
          [fd] "{x10}" (@as(usize, 0)),
          [buf] "{x11}" (@intFromPtr(buf)),
          [count] "{x12}" (count),
        : .{ .memory = true });

    return @bitCast(ret);
}

pub fn syscall_sleep_ms(ms: usize) void {
    asm volatile ("ecall"
        :
        : [number] "{x17}" (Syscall.sleep_ms),
          [ms] "{x10}" (ms),
        : .{ .memory = true });
}

pub fn syscall_shutdown() void {
    asm volatile ("ecall"
        :
        : [number] "{x17}" (Syscall.shutdown),
        : .{ .memory = true });
}

pub fn syscall_exit(exit_code: i32) noreturn {
    asm volatile ("ecall"
        :
        : [number] "{x17}" (Syscall.exit),
          [exit_code] "{x10}" (exit_code),
        : .{ .memory = true });

    while (true) {}
}

pub fn syscall_spawn(
    pid: *usize,
    path: [*]u8,
    argv: [*:null]const ?[*:0]const u8,
) isize {
    const ret = asm volatile ("ecall"
        : [ret] "={x10}" (-> usize),
        : [number] "{x17}" (Syscall.spawn),
          [pid] "{x10}" (@intFromPtr(pid)),
          [path] "{x11}" (@intFromPtr(path)),
          [argv] "{x12}" (@intFromPtr(argv)),
        : .{ .memory = true });

    return @bitCast(ret);
}

pub fn syscall_wait() isize {
    const ret = asm volatile ("ecall"
        : [ret] "={x10}" (-> usize),
        : [number] "{x17}" (Syscall.wait),
        : .{ .memory = true });

    return @bitCast(ret);
}
