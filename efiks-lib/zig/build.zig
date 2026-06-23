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

    const sample_prog = b.addExecutable(.{
        .name = "sample_prog",
        .root_module = b.createModule(.{
            .root_source_file = b.path("src/bin/sample_prog.zig"),
            .target = target,
            .optimize = optimize,
            .single_threaded = true,
            .imports = &.{.{ .name = "efiks", .module = mod }},
        }),
    });

    b.installArtifact(sample_prog);

    const sample_step = b.step("sample-prog", "Build sample Zig program");
    sample_step.dependOn(&b.addInstallArtifact(sample_prog, .{}).step);

    const spawned_prog = b.addExecutable(.{
        .name = "spawned_prog",
        .root_module = b.createModule(.{
            .root_source_file = b.path("src/bin/spawned_prog.zig"),
            .target = target,
            .optimize = optimize,
            .single_threaded = true,
            .imports = &.{.{ .name = "efiks", .module = mod }},
        }),
    });

    b.installArtifact(spawned_prog);

    const spawned_step = b.step("spawned-prog", "Build sample Zig program");
    spawned_step.dependOn(&b.addInstallArtifact(spawned_prog, .{}).step);
}
