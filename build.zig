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

    const efiks_lib_zig = b.dependency("efiks_lib_zig", .{
        .target = target,
        .optimize = optimize,
    });

    const efiks = efiks_lib_zig.artifact("efiks");
    const install_efiks = b.addInstallArtifact(efiks, .{});

    b.getInstallStep().dependOn(&install_efiks.step);

    const efiks_step = b.step("efiks-lib-zig", "Build efiks-lib/zig");
    efiks_step.dependOn(&install_efiks.step);
}
