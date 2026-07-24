const efiks = @import("efiks");
const std = @import("std");

pub const std_options: std.Options = .{
    .signal_stack_size = null,
    .enable_segfault_handler = false,
};

pub fn main(_: std.process.Init.Minimal) !void {
    var buffer: [256]u8 = undefined;
    var writer: std.Io.Writer = .fixed(&buffer);

    try writer.print("hello world from the spawned program\n", .{});
    try writer.flush();

    efiks.syscall_sleep_ms(2000);
}
