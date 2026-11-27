/*
 * Minimal Cortex-M4 startup shared by all Backstep example and test firmware.
 *
 * - Provides the core exception vector table (entries 0..15).
 * - Copies .data from flash to RAM and zeroes .bss before calling main().
 * - Default handlers spin forever so a fault is visible in a debugger.
 *
 * Targets that need device interrupts (IRQ0+) can append their own table
 * entries by defining BACKSTEP_DEVICE_VECTORS (see target sources).
 *
 * SPDX-License-Identifier: MIT
 */
#include <stdint.h>

#include "vectors.h"

extern uint32_t _sidata; /* start of .data initial values in flash */
extern uint32_t _sdata;  /* start of .data in RAM */
extern uint32_t _edata;  /* end of .data in RAM */
extern uint32_t _sbss;   /* start of .bss */
extern uint32_t _ebss;   /* end of .bss */
extern uint32_t _estack; /* initial stack pointer (top of RAM) */

int main(void);

/* Weak aliases: a target overrides only the handlers it needs. */
void NMI_Handler(void) __attribute__((weak, alias("Default_Handler")));
void HardFault_Handler(void) __attribute__((weak, alias("Default_Handler")));
void MemManage_Handler(void) __attribute__((weak, alias("Default_Handler")));
void BusFault_Handler(void) __attribute__((weak, alias("Default_Handler")));
void UsageFault_Handler(void) __attribute__((weak, alias("Default_Handler")));
void SVC_Handler(void) __attribute__((weak, alias("Default_Handler")));
void DebugMon_Handler(void) __attribute__((weak, alias("Default_Handler")));
void PendSV_Handler(void) __attribute__((weak, alias("Default_Handler")));
void SysTick_Handler(void) __attribute__((weak, alias("Default_Handler")));

typedef void (*vector_t)(void);

__attribute__((section(".isr_vector"), used))
const vector_t backstep_core_vectors[16] = {
    (vector_t)(uintptr_t)&_estack,
    Reset_Handler,
    NMI_Handler,
    HardFault_Handler,
    MemManage_Handler,
    BusFault_Handler,
    UsageFault_Handler,
    0, 0, 0, 0, /* reserved */
    SVC_Handler,
    DebugMon_Handler,
    0, /* reserved */
    PendSV_Handler,
    SysTick_Handler,
};

void Reset_Handler(void)
{
    uint32_t *src = &_sidata;
    uint32_t *dst = &_sdata;

    while (dst < &_edata) {
        *dst++ = *src++;
    }
    for (dst = &_sbss; dst < &_ebss; ++dst) {
        *dst = 0u;
    }

    (void)main();

    for (;;) {
        /* main() must not return; park the core if it does. */
    }
}

void Default_Handler(void)
{
    for (;;) {
    }
}
