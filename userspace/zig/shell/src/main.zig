const efiks = @import("efiks");
const std = @import("std");

pub const std_options: std.Options = .{
    .signal_stack_size = null,
    .enable_segfault_handler = false,
};

const PROMPT: []const u8 = "$ ";

pub fn main(init: std.process.Init.Minimal) !void {
    var arena: std.heap.ArenaAllocator = .init(std.heap.brk_allocator);
    defer arena.deinit();

    const allocator = arena.allocator();

    var threaded: std.Io.Threaded = .init(allocator, .{ .argv0 = .init(init.args), .environ = init.environ });
    defer threaded.deinit();

    const io: std.Io = threaded.io();

    try runShell(io, allocator);
}

fn runShell(io: std.Io, _: std.mem.Allocator) !void {
    var readBuffer: [1]u8 = undefined;
    var lineBuffer: [1024]u8 = undefined;
    var writeBuffer: [1024]u8 = undefined;

    var fileReader = std.Io.File.stdin().reader(io, &readBuffer);
    var fileWriter = std.Io.File.stdout().writer(io, &writeBuffer);

    const reader = &fileReader.interface;
    var writer = &fileWriter.interface;

    try writer.writeAll(PROMPT);
    try writer.flush();

    while (true) {
        _ = try readLine(reader, writer, &lineBuffer);
    }
}

fn readLine(reader: *std.Io.Reader, writer: *std.Io.Writer, buffer: []u8) !?[]const u8 {
    var len: usize = 0;

    while (true) {
        const byte = try reader.takeByte();

        switch (byte) {
            // Enter
            '\n', '\r' => {
                try writer.writeAll("\r\n");
                try writer.flush();

                return buffer[0..len];
            },

            // Backspace
            0x08, 0x07f => {
                if (len > 0) {
                    len -= 1;

                    try writer.writeAll("\x08 \x08");
                    try writer.flush();
                }
            },

            // Ctrl-C
            0x03 => {
                len = 0;
                try writer.writeAll("^C\r\n");
                try writer.flush();

                return buffer[0..0];
            },

            else => {
                if (byte < 0x20)
                    continue;

                if (len == buffer.len) {
                    // command is too long
                    try writer.writeAll("\x07");
                    try writer.flush();
                    continue;
                }

                buffer[len] = byte;
                len += 1;

                try writer.writeByte(byte);
                try writer.flush();
            },
        }
    }
}
