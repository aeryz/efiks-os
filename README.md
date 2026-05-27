# Efiks: An experimental general purpose OS

A small operating system written in Rust, built to explore and understand low-level systems design.

## Overview

This project is an experimental OS kernel focused on learning and teaching core operating system concepts. It is written in Rust with an emphasis on correctness, clarity, and explicit control over low-level behavior.

While the architecture is designed to be ISA-independent, the current implementation targets RISC-V.

## Goals

- Build a minimal but realistic OS from first principles
- Understand core subsystems (memory management, scheduling, traps, syscalls)
- Provide a clear, inspectable codebase for educational purposes
- Produce accompanying material explaining design decisions and internals

## Roadmap

- [x] Boot + OpenSBI (M → S mode)
- [x] Trap handling and context switching
- [x] Sv39 virtual memory (per-process address spaces)
- [x] Multicore support (multi-hart)
- [x] Basic process model + round-robin scheduler
- [x] Syscalls (`write`, `read`, `sleep`, `exit`)
- [x] UART driver (console I/O)
- [x] Simple heap allocator
- [x] Basic synchronization primitives (spinlocks)
- [X] VirtIO block driver for persistent storage.
- [x] Basic filesystem support
- [ ] Improve memory management (allocator, paging, regions)
- [ ] Process lifecycle (cleanup, reaper, better scheduling)
- [x] Expand syscalls + userspace support (ELF loader)
- [ ] Improve filesystem (features, robustness)
- [ ] Device support (e.g. VirtIO)
- [ ] Strengthen ISA abstraction (beyond RISC-V)
- [ ] Write accompanying educational content

## Running

Right now, I only have the docs for nix users:

### Enter the devshell
Use `direnv`:
```
direnv allow
```

Or just:
```
nix develop
```

### Build the sbi bootloader

```
nix build .#opensbi
```

### Run the OS
```
RUST_LOG=info cargo b \
  && qemu-system-riscv64 \
      -smp 4 \
      -nographic \
      -machine virt \
      -bios ./result/share/opensbi/lp64/generic/firmware/fw_dynamic.bin \
      -kernel target/riscv64gc-unknown-none-elf/release/kernel
```

## Resources

1. Huge shoutout to the [OSTEP book](https://pages.cs.wisc.edu/~remzi/OSTEP/) that let me grasp most of the OS concepts. It is a very easy to read book so I strongly recommend it. (don't forget to support the author if you can)
2. The blog posts of [Uros Popovic](https://popovicu.com/posts/bare-metal-programming-risc-v/) made it easier to bootstrap the project by explaining the qemu RISC-V internals.
3. The official [RISC-V specification](https://docs.riscv.org/reference/isa/) is very helpful to have the full layout of the registers and basically anything related to the hardware.
4. Very comprehensive [RISC-V course](https://www.youtube.com/watch?v=VEQL5bJeWB0&list=PLbtzT1TYeoMiKup6aoQc3V_d7OvOKc3P5&index=1) by Harry H. Porter (what a cool name). Reading the specification is not easy and can feel a bit dry. This helps you grasp the RISC-V assembly, trap handling, CSR's, etc. It's basically RISC-V spec but for humans.
5. I use ChatGPT only for asking questions about the risc-v spec when I'm stuck. LLMs are great tools for fetching you a specific information out of huge documents. But note that, it certainly won't help to let the AI code for you in this case. The learning comes from suffering.
6. [xv6-kernel documentation by MIT](https://pdos.csail.mit.edu/6.828/2020/xv6/book-riscv-rev1.pdf) I skim through the documentation to see their choice of algorithms. Would be a great source if you prefer to follow this course with it's source code entirely.

## Contribution
I'm not an expert at all. I'm just learning things by doing it. So, feel free to drop an issue if you:
- spot an error,
- think that there is a better way of doing things,
- have any questions. (issue labeled as "question")

