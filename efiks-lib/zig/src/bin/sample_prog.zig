const efiks = @import("efiks");

export fn _start() noreturn {
    while (true) {
        var buf: [1]u8 = undefined;
        const n_read = efiks.read(&buf);
        if (n_read == 0) {
            _ = efiks.write("\n");
        } else if (n_read > 0) {
            switch (buf[0]) {
                127, 8 => _ = efiks.write("\x08 \x08"),
                else => _ = efiks.write(@constCast(&buf)),
            }
        }
    }
}
