/*
 * Test harness binding for QEMU mps2-an386: console on the CMSDK UART0
 * (QEMU's stdout), verdict through the semihosting exit code. Every UART
 * register access goes through these functions, so tests that trap
 * peripheral accesses can still print.
 *
 * SPDX-License-Identifier: MIT
 */
#ifndef BACKSTEP_QEMU_TEST_H
#define BACKSTEP_QEMU_TEST_H

#include <stdint.h>

#include "semihost.h"

#define QT_UART0_BASE 0x40004000u

static inline void test_uart_init(void)
{
    *(volatile uint32_t *)(QT_UART0_BASE + 0x10u) = 16u; /* BAUDDIV */
    *(volatile uint32_t *)(QT_UART0_BASE + 0x08u) = 1u;  /* CTRL: TX enable */
}

static inline void test_putc(char c)
{
    while (*(volatile uint32_t *)(QT_UART0_BASE + 0x04u) & 1u) { /* TXFULL */
    }
    *(volatile uint32_t *)(QT_UART0_BASE + 0x00u) = (uint32_t)(unsigned char)c;
}

static inline void test_platform_finish(int ok)
{
    semihost_exit(ok);
}

#include "test_harness.h"

#endif /* BACKSTEP_QEMU_TEST_H */
