const std = @import("std");

const riscv64_freestanding: std.Target.Query = .{
    .cpu_arch = .riscv64,
    .os_tag = .linux,
    .abi = .gnu,
};

const x86_64_linux: std.Target.Query = .{
    .cpu_arch = .x86_64,
    .os_tag = .linux,
    .abi = .gnu,
};

pub fn build(b: *std.Build) void {
    const target = b.standardTargetOptions(.{
        .default_target = riscv64_freestanding,
    });
    const linux_target = b.resolveTargetQuery(x86_64_linux);

    const optimize = b.standardOptimizeOption(.{});

    // efiks-lib for the primary target, RISC-V freestanding by default.
    const efiks_lib_zig = b.dependency("efiks_lib_zig", .{
        .target = target,
        .optimize = optimize,
    });

    // Shell for the primary target, RISC-V freestanding by default.
    const shell_dep = b.dependency("shell", .{
        .target = target,
        .optimize = optimize,
    });

    // A second instance of the shell dependency targeting x86_64 Linux.
    const shell_linux_dep = b.dependency("shell", .{
        .target = linux_target,
        .optimize = optimize,
    });

    const efiks = efiks_lib_zig.artifact("efiks");
    const shell = shell_dep.artifact("shell");
    const shell_linux = shell_linux_dep.artifact("shell");

    const install_efiks = b.addInstallArtifact(efiks, .{});

    const install_shell = b.addInstallArtifact(shell, .{
        .dest_sub_path = "shell-riscv64",
    });

    const install_shell_linux = b.addInstallArtifact(shell_linux, .{
        .dest_sub_path = "shell-x86_64-linux",
    });

    // `zig build` installs all three artifacts.
    const install_step = b.getInstallStep();
    install_step.dependOn(&install_efiks.step);
    install_step.dependOn(&install_shell.step);
    install_step.dependOn(&install_shell_linux.step);

    // `zig build efiks-lib-zig`
    const efiks_step = b.step(
        "efiks-lib-zig",
        "Build efiks-lib/zig",
    );
    efiks_step.dependOn(&install_efiks.step);

    // `zig build shell`
    const shell_step = b.step(
        "shell",
        "Build userspace Zig shell for RISC-V",
    );
    shell_step.dependOn(&install_shell.step);

    // `zig build shell-linux`
    const shell_linux_step = b.step(
        "shell-linux",
        "Build userspace Zig shell for x86_64 Linux",
    );
    shell_linux_step.dependOn(&install_shell_linux.step);

    // `zig build shells`
    const shells_step = b.step(
        "shells",
        "Build both userspace Zig shell targets",
    );
    shells_step.dependOn(&install_shell.step);
    shells_step.dependOn(&install_shell_linux.step);

    const efiks_check = b.addLibrary(.{
        .name = "efiks-check",
        .root_module = efiks_lib_zig.module("efiks"),
    });

    const shell_check = b.addExecutable(.{
        .name = "shell-riscv64-check",
        .root_module = shell_dep.module("shell"),
    });

    const shell_linux_check = b.addExecutable(.{
        .name = "shell-x86_64-linux-check",
        .root_module = shell_linux_dep.module("shell"),
    });

    // `zig build check`
    const check = b.step("check", "Check workspace");
    check.dependOn(&efiks_check.step);
    check.dependOn(&shell_check.step);
    check.dependOn(&shell_linux_check.step);
}
