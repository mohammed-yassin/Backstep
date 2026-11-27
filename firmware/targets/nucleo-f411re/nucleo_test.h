/*
 * Test harness binding for NUCLEO-F411RE: console on USART2 (PA2), which
 * the on-board ST-LINK exposes as a USB virtual COM port at 115200 8N1,
 * running from the 16 MHz HSI reset clock. After the verdict the core
 * idles; the host script reads the result from the console.
 *
 * SPDX-License-Identifier: MIT
 */
#ifndef BACKSTEP_NUCLEO_TEST_H
#define BACKSTEP_NUCLEO_TEST_H

#include <stdint.h>

#define NT_REG32(a) (*(volatile uint32_t *)(a))
#define NT_RCC_AHB1ENR NT_REG32(0x40023830u)
#define NT_RCC_APB1ENR NT_REG32(0x40023840u)
#define NT_GPIOA_MODER NT_REG32(0x40020000u)
#define NT_GPIOA_AFRL NT_REG32(0x40020020u)
#define NT_USART2_SR NT_REG32(0x40004400u)
#define NT_USART2_DR NT_REG32(0x40004404u)
#define NT_USART2_BRR NT_REG32(0x40004408u)
#define NT_USART2_CR1 NT_REG32(0x4000440Cu)

static inline void test_uart_init(void)
{
    NT_RCC_AHB1ENR |= 1u << 0;  /* GPIOA */
    NT_RCC_APB1ENR |= 1u << 17; /* USART2 */
    (void)NT_RCC_APB1ENR;
    NT_GPIOA_MODER = (NT_GPIOA_MODER & ~(3u << 4)) | (2u << 4); /* PA2 AF */
    NT_GPIOA_AFRL = (NT_GPIOA_AFRL & ~(0xFu << 8)) | (7u << 8);  /* AF7 */
    NT_USART2_BRR = 0x8Bu; /* 115200 baud from 16 MHz */
    NT_USART2_CR1 = (1u << 13) | (1u << 3); /* UE | TE */
}

static inline void test_putc(char c)
{
    if (c == '\n') {
        while ((NT_USART2_SR & (1u << 7)) == 0u) { /* TXE */
        }
        NT_USART2_DR = '\r';
    }
    while ((NT_USART2_SR & (1u << 7)) == 0u) {
    }
    NT_USART2_DR = (uint32_t)(unsigned char)c;
}

static inline void test_platform_finish(int ok)
{
    (void)ok;
    for (;;) {
        __asm__ volatile("wfi");
    }
}

#include "test_harness.h"

#endif /* BACKSTEP_NUCLEO_TEST_H */
