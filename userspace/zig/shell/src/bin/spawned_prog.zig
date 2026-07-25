const std = @import("std");

pub const std_options: std.Options = .{
    .signal_stack_size = null,
    .enable_segfault_handler = false,
};

var probe: [8192]u8 align(4096) = [_]u8{0} ** 8192;

fn say(message: []const u8) void {
    _ = std.os.linux.write(1, message.ptr, message.len);
}

fn fail(message: []const u8) noreturn {
    say(message);
    std.os.linux.exit(1);
}

pub fn main(_: std.process.Init.Minimal) !void {
    const page0: *volatile u8 = @ptrCast(&probe[0]);
    const page1: *volatile u8 = @ptrCast(&probe[4096]);

    page0.* = 0x11;
    page1.* = 0x22;

    const fork_result = std.os.linux.fork();
    if (std.os.linux.errno(fork_result) != .SUCCESS) {
        fail("fork failed\n");
    }

    if (fork_result == 0) {
        if (page0.* != 0x11 or page1.* != 0x22) {
            fail("child inherited incorrect data\n");
        }

        // These writes should trigger exactly one CoW fault.
        page0.* = 0x33;
        page0.* = 0x44;

        if (page0.* != 0x44 or page1.* != 0x22) {
            fail("child CoW write failed\n");
        }

        say("child: CoW write passed\n");
        std.os.linux.exit(0);
    }

    var status: u32 = 0;
    const wait_result = std.os.linux.waitpid(@intCast(fork_result), &status, 0);
    if (std.os.linux.errno(wait_result) != .SUCCESS) {
        fail("waitpid failed\n");
    }

    if (!std.os.linux.W.IFEXITED(status) or std.os.linux.W.EXITSTATUS(status) != 0) {
        fail("child failed\n");
    }

    if (page0.* != 0x11 or page1.* != 0x22) {
        fail("parent data was modified by child\n");
    }

    // The parent mapping is still read-only after the child exits. This tests
    // the final-owner CoW path and release of the old frame.
    page0.* = 0x55;
    if (page0.* != 0x55) {
        fail("parent CoW write failed\n");
    }

    say("parent: CoW isolation passed\n");
}
