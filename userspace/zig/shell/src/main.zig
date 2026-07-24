const efiks = @import("efiks");
const std = @import("std");

pub const std_options: std.Options = .{
    .signal_stack_size = null,
    .enable_segfault_handler = false,
};

const PROMPT: []const u8 = "$ ";

const ShellError = error{
    InvalidArgs,
};

pub fn main(init: std.process.Init.Minimal) !void {
    var arena: std.heap.ArenaAllocator = .init(std.heap.brk_allocator);
    defer arena.deinit();

    const allocator = arena.allocator();

    var threaded: std.Io.Threaded = .init(allocator, .{ .argv0 = .init(init.args), .environ = init.environ });
    defer threaded.deinit();

    const io: std.Io = threaded.io();

    try runShell(io, allocator);
}

fn runShell(io: std.Io, allocator: std.mem.Allocator) !void {
    var readBuffer: [1]u8 = undefined;
    var lineBuffer: [1024]u8 = undefined;
    var writeBuffer: [1024]u8 = undefined;

    var fileReader = std.Io.File.stdin().reader(io, &readBuffer);
    var fileWriter = std.Io.File.stdout().writer(io, &writeBuffer);

    const reader = &fileReader.interface;
    var writer = &fileWriter.interface;

    while (true) {
        try writer.writeAll(PROMPT);
        try writer.flush();

        const cmd = try readLine(reader, writer, &lineBuffer);

        const trimmedCmd = std.mem.trim(u8, cmd.?, " \t");

        var it = std.mem.splitAny(u8, trimmedCmd, " \t");

        if (it.next()) |prog| {
            if (runProg(reader, writer, prog, &it, allocator)) |_| {} else |err| {
                try writer.print("error: {}\n", .{err});
                try writer.flush();
            }
        }
    }
}

fn runProg(_: *std.Io.Reader, writer: *std.Io.Writer, prog: []const u8, it: *std.mem.SplitIterator(u8, std.mem.DelimiterType.any), allocator: std.mem.Allocator) !void {
    if (std.mem.eql(u8, prog, "spawn")) {
        const path = try allocator.dupeZ(u8, getNextNonSpace(it) orelse return ShellError.InvalidArgs);
        defer allocator.free(path);

        var pid: usize = 0;

        var args = try std.ArrayList(?[*:0]const u8).initCapacity(allocator, 1);
        defer {
            for (args.items) |arg| {
                if (arg) |ptr| allocator.free(std.mem.span(ptr));
            }
            args.deinit(allocator);
        }

        while (getNextNonSpace(it)) |arg| {
            const spawnArg: [:0]u8 = try allocator.dupeZ(u8, arg);
            errdefer allocator.free(spawnArg);

            try args.append(allocator, spawnArg);
        }

        const argv = try args.toOwnedSliceSentinel(allocator, null);
        defer {
            for (argv) |arg| {
                allocator.free(std.mem.span(arg.?));
            }
            allocator.free(argv);
        }

        const err = efiks.syscall_spawn(&pid, @ptrCast(path), argv);
        if (err < 0) {
            try writer.print("spawn: exited with err {}\n", .{err});
            try writer.flush();
        }
    }
}

fn getNextNonSpace(it: *std.mem.SplitIterator(u8, std.mem.DelimiterType.any)) ?[]const u8 {
    while (it.next()) |value| {
        const trimmed = std.mem.trim(u8, value, " \t");
        if (trimmed.len > 0) {
            return trimmed;
        }
    }

    return null;
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
