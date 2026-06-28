const std = @import("std");

const riscv64_freestanding: std.Target.Query = .{
    .cpu_arch = .riscv64,
    .os_tag = .freestanding,
    .abi = .none,
};

pub fn build(b: *std.Build) void {
    const target = b.standardTargetOptions(.{
        .default_target = riscv64_freestanding,
    });
    const optimize = b.standardOptimizeOption(.{});

    const mod = b.addModule("efiks", .{
        .root_source_file = b.path("src/root.zig"),
        .target = target,
        .optimize = optimize,
        .single_threaded = true,
    });

    const lib = b.addLibrary(.{
        .name = "efiks",
        .root_module = mod,
    });

    b.installArtifact(lib);
}
