/*
 * Linker script for QEMU "mps2-an386" (Arm MPS2 FPGA image AN386, Cortex-M4).
 * ZBT SSRAM1 at 0x00000000 holds code; SSRAM2/3 at 0x20000000 holds data.
 *
 * SPDX-License-Identifier: MIT
 */
ENTRY(Reset_Handler)

MEMORY
{
    FLASH (rx)  : ORIGIN = 0x00000000, LENGTH = 4M
    RAM   (rwx) : ORIGIN = 0x20000000, LENGTH = 4M
}

_estack = ORIGIN(RAM) + LENGTH(RAM);

SECTIONS
{
    .isr_vector :
    {
        KEEP(*(.isr_vector))        /* core exceptions 0-15 (startup) */
        KEEP(*(.isr_vector.device)) /* device IRQs 16+ (firmware) */
    } > FLASH

    .text :
    {
        *(.text*)
        *(.rodata*)
        KEEP(*(.backstep.keep*)) /* entry points used only by a debugger */
        . = ALIGN(4);
    } > FLASH

    _sidata = LOADADDR(.data);

    .data :
    {
        . = ALIGN(4);
        _sdata = .;
        *(.data*)
        . = ALIGN(4);
        _edata = .;
    } > RAM AT > FLASH

    .bss (NOLOAD) :
    {
        . = ALIGN(4);
        _sbss = .;
        *(.bss*)
        *(COMMON)
        . = ALIGN(4);
        _ebss = .;
    } > RAM

    /* Backstep's own working memory (snapshot headers, logs). Kept out of
     * [_sdata, _ebss) so a snapshot of the program's RAM excludes it. */
    .backstep (NOLOAD) :
    {
        . = ALIGN(4);
        *(.backstep*)
        . = ALIGN(4);
    } > RAM

    /DISCARD/ : { *(.ARM.exidx*) }
}
