set architecture riscv:rv64
set disassemble-next-line on
target remote :1234
set confirm off
file target/riscv64gc-unknown-none-elf/debug/kernel
set confirm on
