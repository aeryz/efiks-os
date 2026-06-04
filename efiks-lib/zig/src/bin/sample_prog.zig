const efiks = @import("efiks");

const PROMPT: []const u8 = "shell $ ";

export fn _start() noreturn {
    while (true) {
        var buf: [512]u8 = @splat(0);
        var pos: usize = 0;

        _ = efiks.write(PROMPT);

        while (buf[pos] != '\n' and buf[pos] != '\r') {
            const n_read = efiks.read(buf[pos..(pos + 1)]);
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
                    if (pos >= buf.len) {
                        break;
                    }
                    _ = efiks.write(buf[pos..]);
                    pos += 1;
                },
            }
        }

        const cmd = buf[0..pos];
        if (eql(cmd, "help")) {
            _ = efiks.write("available commands:\n");
            _ = efiks.write("- spawn\n");
        } else if (cmd.len > 6 and eql(cmd[0..6], "spawn ")) {
            buf[pos] = 0;
            const path = cmd[6..];
            var pid: usize = undefined;
            _ = efiks.syscall_spawn(@ptrCast(path), &pid);
        }
    }
}

fn eql(a: []const u8, b: []const u8) bool {
    if (a.len != b.len) return false;

    for (a, b) |x, y| {
        if (x != y) return false;
    }

    return true;
}
