# efiks-os

**A from-scratch, Unix-like operating system for RISC-V, written primarily in Rust.**

`efiks-os` is an experimental general-purpose kernel built to explore how a modern operating system works beneath its abstractions.

It boots through OpenSBI, runs across multiple RISC-V harts, provides isolated Sv39 address spaces, loads ELF programs, schedules userspace processes, supports copy-on-write process creation, exposes a growing Linux-compatible syscall interface, and persists files through a custom filesystem running over VirtIO block devices.

The project is developed as a real systems-engineering exercise rather than as a thin wrapper around an existing kernel or a direct port of xv6. Its main purpose is to build and understand the difficult connective tissue between virtual memory, process execution, scheduling, storage, synchronization and userspace ABIs.

> **Project status:** efiks-os is under active development. It is substantial enough to run selected userspace programs, but it is not a production operating system and does not claim full Linux or POSIX compatibility.

## Current capabilities

### Architecture and boot

* 64-bit RISC-V target
* OpenSBI boot flow
* Supervisor-mode kernel
* QEMU `virt` machine support
* Multi-hart initialization and execution
* Architecture-specific code isolated behind dedicated RISC-V abstractions
* Trap, exception and interrupt handling
* Timer-driven preemption
* Context switching between kernel and userspace execution

### Virtual memory

* Sv39 virtual memory
* Separate userspace address spaces
* Kernel direct mapping of physical memory
* Page-table creation and traversal
* User and kernel address abstractions
* Typed user and kernel pointers
* Virtual-memory-region tracking
* Demand allocation through page faults
* Userspace heap growth through `brk`
* Copy-on-write address-space cloning
* Physical-page metadata and reference counting
* User-memory copy and validation helpers
* Per-process user stacks
* Per-task kernel stacks

### Processes and scheduling

* Preemptive multitasking
* Multicore scheduling
* Kernel and userspace tasks
* Process IDs
* Task lifecycle states
* Process creation from ELF executables
* Parent and child process relationships
* `fork`-style process creation through a supported subset of Linux `clone`
* Copy-on-write process memory
* Child trap-frame construction
* File-table inheritance
* Process exit and zombie states
* Waiting for child processes
* Deferred task and address-space cleanup
* Kernel reaper task
* Sleeping tasks organized by wake-up deadline
* Blocking and wake-up paths for scheduler-integrated operations

### Userspace execution

* ELF64 loading
* User/kernel privilege transitions
* Userspace stack construction
* `argc` and `argv` setup
* Syscall entry and return
* A growing Linux RISC-V syscall ABI subset
* Selected Zig programs built for Linux running directly on efiks-os
* Zig userspace support library
* Interactive Zig shell
* Userspace process spawning and waiting
* Standard-library-driven I/O paths such as vectored reads and writes

Linux ABI compatibility is intentionally incremental. A matching syscall number does not imply that every Linux flag, edge case or semantic guarantee is implemented.

### Files and storage

* Virtual filesystem layer
* File-descriptor tables
* Standard input, output and error descriptors
* File open, read, write and close paths
* Vectored I/O
* Positional vectored I/O
* File offset handling
* Persistent block storage
* VirtIO block-device support
* Custom filesystem: **VSFS**
* Host-side VSFS image creation tool
* Directory and inode abstractions
* Block and inode allocation bitmaps
* Files spanning multiple blocks
* Allocation rollback and cleanup paths
* Synchronization around persistent allocation metadata

### Devices and kernel infrastructure

* UART console driver
* VirtIO MMIO support
* VirtIO block driver
* Timer interrupts
* External interrupt handling
* Spinlocks and synchronization primitives
* Kernel heap allocator
* Physical frame allocator
* Structured logging
* GDB support and helper tooling
* Nix development environment

---

## Example userspace flow

A typical userspace process crosses most of the kernel:

```text
VSFS file
   │
   ▼
VFS lookup
   │
   ▼
ELF loader
   │
   ▼
new Sv39 address space
   │
   ▼
user stack and argv
   │
   ▼
scheduler
   │
   ▼
userspace execution
   │
   ▼
Linux-compatible syscall entry
   │
   ├── file descriptor and VFS operations
   ├── sleeping and blocking
   ├── process creation
   ├── virtual-memory changes
   └── process exit and reaping
```

A process created through `clone` exercises another cross-subsystem path:

```text
parent process
   │
   ├── clone address space using copy-on-write
   ├── share physical pages through reference counts
   ├── remove writable access from shared mappings
   ├── copy execution and trap state
   ├── inherit file descriptors
   ├── establish parent/child relationships
   └── enqueue child into the scheduler
              │
              ▼
       write page fault
              │
              ▼
       allocate private page
              │
              ▼
       copy original contents
              │
              ▼
       resume child execution
```

---

## Running efiks-os

The documented development path currently uses Nix, Cargo, Zig and QEMU.

### Requirements

* Nix with flakes enabled
* Rust toolchain supplied by the development shell
* Zig toolchain supplied by the development shell
* QEMU with RISC-V system emulation

### Enter the development environment

Using `direnv`:

```console
direnv allow
```

Or directly:

```console
nix develop
```

### Build OpenSBI

```console
nix build .#opensbi
```

### Create a `VSFS` image

```
./scripts/build-disk.sh
```

### Build and boot the kernel

```console
RUST_LOG=info cargo build -p kernel \
  && qemu-system-riscv64 \
    -smp 4 \
    -nographic \
    -machine virt \
    -bios ./result/share/opensbi/lp64/generic/firmware/fw_dynamic.bin \
    -kernel target/riscv64gc-unknown-none-elf/debug/kernel \
    -drive file=disk.img,format=raw,if=none,id=blk0,cache=none \
    -device virtio-blk-device,drive=blk0 \
    -global virtio-mmio.force-legacy=false
```

Useful QEMU options during development include:

```console
-s -S
```

These expose a GDB server on port `1234` and pause the machine before execution.

---

## Current limitations

efiks-os remains experimental.

Notable limitations include:

* only RISC-V is currently supported,
* QEMU `virt` is the primary platform,
* Linux syscall compatibility is incomplete,
* POSIX behavior is incomplete,
* dynamically linked Linux binaries are not a general compatibility target yet,
* the process and thread models are not fully separated,
* signal support is limited or absent,
* networking is not implemented,
* filesystem crash consistency is not production-grade,
* security hardening is not a current claim,
* device coverage is intentionally small,
* and many resource and concurrency paths still need stress testing.

Unsupported functionality should generally fail explicitly rather than silently pretending to provide full Linux semantics.

---

## Debugging

The repository includes GDB configuration and helper tooling under:

```text
tools/gdb/
```

A typical debugging session starts QEMU paused with a GDB server:

```console
qemu-system-riscv64 \
  -s \
  -S \
  ...
```

Then connect using a RISC-V-aware GDB build:

```console
target remote :1234
```

The exact debugger command depends on the toolchain available in the Nix development environment.

---

## Learning resources

The following resources have been especially useful during development:

1. **Operating Systems: Three Easy Pieces**
   A clear introduction to virtual memory, scheduling, concurrency, persistence and other core operating-system concepts.

2. **The RISC-V privileged and unprivileged specifications**
   The authoritative reference for privilege modes, CSRs, traps, address translation and instruction behavior.

3. **Uros Popovic’s RISC-V and QEMU articles**
   Helpful material for understanding early bootstrapping and QEMU’s RISC-V environment.

4. **Harry H. Porter’s RISC-V material**
   A more approachable companion to the architectural specifications.

5. **MIT xv6 and its accompanying book**
   A valuable reference for studying one coherent implementation of Unix-like kernel concepts.

These resources are references, not templates for the complete efiks-os design.

---

## License

efiks-os is licensed under the **GNU General Public License v3.0**.

See [`LICENSE`](LICENSE) for the complete license text.
