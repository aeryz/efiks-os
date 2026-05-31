pub const SYSCALL_WRITE: usize = 1;
pub const SYSCALL_READ: usize = 2;
pub const SYSCALL_SLEEP_MS: usize = 3;
pub const SYSCALL_SHUTDOWN: usize = 4;
pub const SYSCALL_EXIT: usize = 5;

pub inline fn write(buf: []const u8) isize {
    return syscall_write(buf.ptr, buf.len);
}

pub inline fn read(buf: []u8) isize {
    return syscall_read(buf.ptr, buf.len);
}

pub fn syscall_write(data_ptr: [*]const u8, len: usize) isize {
    const ret = asm volatile ("ecall"
        : [ret] "={x10}" (-> usize),
        : [number] "{x17}" (SYSCALL_WRITE),
          [fd] "{x10}" (@as(usize, 1)),
          [data_ptr] "{x11}" (@intFromPtr(data_ptr)),
          [len] "{x12}" (len),
        : .{ .memory = true });

    return @bitCast(ret);
}

pub fn syscall_read(buf: [*]u8, count: usize) isize {
    const ret = asm volatile ("ecall"
        : [ret] "={x10}" (-> usize),
        : [number] "{x17}" (SYSCALL_READ),
          [fd] "{x10}" (@as(usize, 0)),
          [buf] "{x11}" (@intFromPtr(buf)),
          [count] "{x12}" (count),
        : .{ .memory = true });

    return @bitCast(ret);
}

pub fn syscall_sleep_ms(ms: usize) void {
    asm volatile ("ecall"
        :
        : [number] "{x17}" (SYSCALL_SLEEP_MS),
          [ms] "{x10}" (ms),
        : .{ .memory = true });
}

pub fn syscall_shutdown() void {
    asm volatile ("ecall"
        :
        : [number] "{x17}" (SYSCALL_SHUTDOWN),
        : .{ .memory = true });
}

pub fn syscall_exit(exit_code: i32) void {
    asm volatile ("ecall"
        :
        : [number] "{x17}" (SYSCALL_EXIT),
          [exit_code] "{x10}" (exit_code),
        : .{ .memory = true });
}
