OUTPUT_ARCH(riscv)
ENTRY(_start)

KERNEL_VMA = 0xffffffff80200000;
KERNEL_LMA = 0x80200000;
LOAD_OFFSET = KERNEL_VMA - KERNEL_LMA;

SECTIONS
{
  . = KERNEL_VMA;

  .text : AT(ADDR(.text) - LOAD_OFFSET) {
    __text_start = .;
    KEEP(*(.text.kernel))
    *(.text .text.*)
    __text_end = .;
  }

  .rodata : AT(ADDR(.rodata) - LOAD_OFFSET) {
    *(.rodata .rodata.*)
  }

  .data : AT(ADDR(.data) - LOAD_OFFSET) {
    *(.data .data.*)
  }

  .bss : AT(ADDR(.bss) - LOAD_OFFSET) {
    *(.bss .bss.*)
    *(COMMON)
  }

  . = ALIGN(16);
  __kernel_stack_bottom = .;
  . = . + 0x10000;     /* reserve 64K */
  __kernel_stack_top = .;

  . = ALIGN(16);
  __stack_bottom = .;
  . = . + 0x10000;     /* reserve 64K */
  __stack_top = .;

  . = ALIGN(4K);
  __kernel_end = .;
}
