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

    const shell = b.addExecutable(.{
        .name = "shell",
        .root_module = b.createModule(.{
            .root_source_file = b.path("src/main.zig"),
            .target = target,
            .optimize = optimize,
            .single_threaded = true,
            .imports = &.{.{ .name = "efiks", .module = efiks_lib_zig.module("efiks") }},
        }),
    });

    b.installArtifact(shell);

    const spawned_prog = b.addExecutable(.{
        .name = "spawned_prog",
        .root_module = b.createModule(.{
            .root_source_file = b.path("src/bin/spawned_prog.zig"),
            .target = target,
            .optimize = optimize,
            .single_threaded = true,
            .imports = &.{.{ .name = "efiks", .module = efiks_lib_zig.module("efiks") }},
        }),
    });

    b.installArtifact(spawned_prog);

    const spawned_step = b.step("spawned-prog", "Build spawned Zig program");
    spawned_step.dependOn(&b.addInstallArtifact(spawned_prog, .{}).step);
}
