const efiks = @import("efiks");
const std = @import("std");

const PROMPT: []const u8 = "shell $ ";
const MAX_SPAWN_ARGS = 8;

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

fn main(_: usize, _: [*]const ?[*:0]const u8) i32 {
    while (true) {
        var buf: [1024]u8 = @splat(0);
        var pos: usize = 0;

        _ = efiks.write(PROMPT);

        while (buf[pos] != '\n' and buf[pos] != '\r') {
            const n_read = efiks.read(0, buf[pos..(pos + 1)]);
            if (n_read == 0) {
                _ = efiks.write("\n");
                break;
            }

            switch (buf[pos]) {
                127, 8 => {
                    if (pos > 0) {
                        buf[pos] = 0;
                        pos -= 1;

                        _ = efiks.write("\x08 \x08");
                    }
                },
                else => {
                    if (pos + 1 >= buf.len) {
                        break;
                    }
                    _ = efiks.write(buf[pos .. pos + 1]);
                    pos += 1;
                },
            }
        }

        const cmd = buf[0..pos];
        if (std.mem.eql(u8, cmd, "help")) {
            _ = efiks.write("available commands:\n");
            _ = efiks.write("- spawn PATH argv0 argv1 ...\n");
        } else if (std.mem.eql(u8, cmd, "spawn") or
            (cmd.len > 6 and std.mem.eql(u8, cmd[0..6], "spawn ")))
        {
            buf[pos] = 0;

            var pid: usize = 0;
            var a: [MAX_SPAWN_ARGS + 1:null]?[*:0]u8 = @splat(null);

            const spawn_args = parse_spawn_command(buf[5 .. pos + 1], a[0..]) orelse {
                _ = efiks.write("usage: spawn PATH argv0 argv1 ...\n");
                continue;
            };
            const argv = a[0..spawn_args.argc :null];

            _ = efiks.syscall_spawn(
                &pid,
                @ptrCast(spawn_args.path),
                argv.ptr,
            );
            if (pid != 0) {
                const res = efiks.syscall_wait();
                var b: [128]u8 = undefined;

                const str = std.fmt.bufPrint(&b, "child finished {} finished execution with code {}\n", .{ res.pid, res.term.exited }) catch unreachable;
                _ = efiks.write(@constCast(str));
            }
        } else if (cmd.len > 3 and std.mem.eql(u8, cmd[0..4], "cat ")) {
            const fd = efiks.open(@ptrCast(cmd[4..]), 0);
            var b: [128]u8 = undefined;
            if (fd < 0) {
                const str = std.fmt.bufPrint(&b, "cat: failed to open file (err: {})\n", .{fd}) catch unreachable;
                _ = efiks.write(@constCast(str));
                continue;
            }
            defer {
                _ = efiks.syscall_close(@intCast(fd));
            }

            while (true) {
                const n_read = efiks.read(@intCast(fd), &buf);
                if (n_read <= 0) {
                    if (n_read != 0) {
                        const in_str = std.fmt.bufPrint(&b, "cat: read failed: {}\n", .{n_read}) catch unreachable;
                        _ = efiks.write(@constCast(in_str));
                    }
                    break;
                }

                _ = efiks.write(@constCast(buf[0..@intCast(n_read)]));
            }
        } else if (std.mem.eql(u8, cmd, "exit")) {
            _ = efiks.syscall_exit(0);
        } else {
            _ = efiks.write("command ");
            _ = efiks.write(cmd);
            _ = efiks.write(" not found.\n");
        }
    }

    return 0;
}

const SpawnArgs = struct {
    path: [*:0]u8,
    argc: usize,
};

fn parse_spawn_command(
    buf: []u8,
    argv_out: []?[*:0]u8,
) ?SpawnArgs {
    var i: usize = skip_spaces(buf, 0);
    if (i >= buf.len or buf[i] == 0) return null;

    const path_start = i;
    i = skip_word(buf, i);
    if (i < buf.len) {
        buf[i] = 0;
        i += 1;
    }

    var argc: usize = 0;
    while (true) {
        i = skip_spaces(buf, i);
        if (i >= buf.len or buf[i] == 0) break;

        if (argc + 1 >= argv_out.len) {
            argv_out[argc] = null;
            break;
        }

        argv_out[argc] = @ptrCast(&buf[i]);
        argc += 1;

        i = skip_word(buf, i);
        if (i < buf.len) {
            buf[i] = 0;
            i += 1;
        }
    }

    argv_out[argc] = null;

    return .{
        .path = @ptrCast(&buf[path_start]),
        .argc = argc,
    };
}

fn skip_spaces(buf: []const u8, start: usize) usize {
    var i = start;
    while (i < buf.len and buf[i] == ' ') : (i += 1) {}
    return i;
}

fn skip_word(buf: []const u8, start: usize) usize {
    var i = start;
    while (i < buf.len and buf[i] != 0 and buf[i] != ' ') : (i += 1) {}
    return i;
}
