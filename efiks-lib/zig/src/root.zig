const std = @import("std");

pub const BrkAllocator = @import("brk_allocator.zig");

const Syscall = enum(usize) { write = 1, read, sleep_ms, shutdown, exit, spawn, wait, open };

pub inline fn write(buf: []const u8) isize {
    return syscall_write(buf.ptr, buf.len);
}

pub inline fn read(fd: usize, buf: []u8) isize {
    return syscall_read(fd, buf.ptr, buf.len);
}

pub inline fn open(path: [*]u8, flags: u32) isize {
    return syscall_open(path, flags);
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

pub fn syscall_read(fd: usize, buf: [*]u8, count: usize) isize {
    const ret = asm volatile ("ecall"
        : [ret] "={x10}" (-> usize),
        : [number] "{x17}" (Syscall.read),
          [fd] "{x10}" (fd),
          [buf] "{x11}" (@intFromPtr(buf)),
          [count] "{x12}" (count),
        : .{ .memory = true });

    return @bitCast(ret);
}

pub fn syscall_open(path: [*]u8, flags: u32) isize {
    const ret = asm volatile ("ecall"
        : [ret] "={x10}" (-> usize),
        : [number] "{x17}" (Syscall.open),
          [path] "{x10}" (@intFromPtr(path)),
          [flags] "{x11}" (flags),
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

pub const WaitResult = struct {
    // TODO(aeryz): pid type
    pid: isize,
    term: std.process.Child.Term,
};

pub fn syscall_wait() WaitResult {
    var wstatus: u16 = undefined;
    const ret = asm volatile ("ecall"
        : [ret] "={x10}" (-> usize),
        : [number] "{x17}" (Syscall.wait),
          [wstatus] "{x10}" (@intFromPtr(&wstatus)),
        : .{ .memory = true });

    const pid: isize = @bitCast(ret);

    return .{
        .pid = pid,
        .term = wstatus_to_term(wstatus),
    };
}

fn wstatus_to_term(wstatus: ?u16) std.process.Child.Term {
    if (wstatus) |status| {
        if (status & 0xFF == 0) {
            return .{ .exited = @intCast((status >> 8) & 0xFF) };
        }
    }

    return .{ .unknown = 0 };
}
