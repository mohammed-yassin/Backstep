/*
 * Linker script for STM32F411RE (NUCLEO-F411RE): 512 KB flash, 128 KB SRAM.
 *
 * SPDX-License-Identifier: MIT
 */
ENTRY(Reset_Handler)

MEMORY
{
    FLASH (rx)  : ORIGIN = 0x08000000, LENGTH = 512K
    RAM   (rwx) : ORIGIN = 0x20000000, LENGTH = 128K
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
        _sbackstep = .;
        *(.backstep*)
        . = ALIGN(4);
        _ebackstep = .;
    } > RAM

    /* The firmware image in flash: vector table, code, .data initialisers.
     * A recording stores its CRC-32 so replay can check the ELF matches. */
    __backstep_image_start = ORIGIN(FLASH);
    __backstep_image_end = LOADADDR(.data) + SIZEOF(.data);

    /DISCARD/ : { *(.ARM.exidx*) }
}
