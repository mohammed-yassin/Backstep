/*
 * Deterministic peripheral-read workload. See capture_workload.h.
 * Written CMSIS-style (register structs behind volatile pointers), as
 * vendor drivers are. Compiled by GCC for the MPU-trap image and by Clang
 * with the Backstep pass for the compile-time capture image.
 *
 * SPDX-License-Identifier: MIT
 */
#include "capture_workload.h"

typedef struct {
    volatile uint32_t CTRL;
    volatile uint32_t VALUE;
    volatile uint32_t RELOAD;
    volatile uint32_t INTSTATUS;
} cmsdk_timer_regs;

typedef struct {
    volatile uint32_t DATA;
    volatile uint32_t STATE;
    volatile uint32_t CTRL;
    volatile uint32_t INTSTATUS;
    volatile uint32_t BAUDDIV;
} cmsdk_uart_regs;

#define TIMER0 ((cmsdk_timer_regs *)0x40000000u)
#define UART0 ((cmsdk_uart_regs *)0x40004000u)

uint32_t workload_peripheral(uint32_t *checksum)
{
    uint32_t i, reads = 0u, acc = 0u;

    for (i = 0; i < 64u; ++i) {
        uint32_t a, b;
        TIMER0->RELOAD = (i * 0x01010101u) ^ 0xA5A5A5A5u;
        a = TIMER0->RELOAD;
        b = TIMER0->RELOAD;
        reads += 2u;
        acc = (acc * 31u) + a + (b >> 3);
        if (a & 1u) {
            acc ^= UART0->BAUDDIV;
            reads++;
        }
        if ((UART0->CTRL & 1u) != 0u) { /* TX enabled by the test harness */
            acc += 7u;
        }
        reads++;
    }
    *checksum = acc;
    return reads;
}

uint32_t workload_ram(uint32_t base, uint32_t *checksum)
{
    volatile uint8_t *u8 = (volatile uint8_t *)base;
    volatile int8_t *s8 = (volatile int8_t *)(base + 0x100u);
    volatile uint16_t *u16 = (volatile uint16_t *)(base + 0x200u);
    volatile int16_t *s16 = (volatile int16_t *)(base + 0x400u);
    volatile uint32_t *u32 = (volatile uint32_t *)(base + 0x800u);
    uint32_t i, reads = 0u;
    int32_t acc = 0;

    for (i = 0; i < 64u; ++i) {
        u8[i] = (uint8_t)(i * 37u);
        s8[i] = (int8_t)(0x80u + i * 3u);
        u16[i] = (uint16_t)(i * 4099u);
        s16[i] = (int16_t)(0x8000u + i * 1031u);
        u32[i] = i * 0x9E3779B9u;
    }
    for (i = 0; i < 64u; ++i) {
        acc += u8[i];
        acc ^= s8[63u - i];
        acc += u16[i] >> 1;
        acc -= s16[i];
        acc += (int32_t)(u32[i] >> 7);
        reads += 5u;
    }
    *checksum = (uint32_t)acc;
    return reads;
}

uint32_t workload_repeat_reads(uint32_t n)
{
    uint32_t i, sum = 0u;
    for (i = 0; i < n; ++i) {
        sum += TIMER0->RELOAD;
    }
    return sum;
}
