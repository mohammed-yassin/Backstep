/*
 * M0 hardware smoke test for NUCLEO-F411RE.
 *
 * - Toggles the green user LED (LD2, PA5) every 500 ms.
 * - Prints a banner and a heartbeat line once per second on USART2
 *   (PA2, AF7), which the on-board ST-LINK exposes as a USB virtual COM
 *   port at 115200 8N1.
 * - Runs from the 16 MHz HSI reset clock; no clock tree setup needed.
 *
 * Expected serial output (see scripts/hil/m0_nucleo_f411re.py):
 *   BACKSTEP-M0 NUCLEO-F411RE blinky recorder=<version> crc=CBF43926
 *   BACKSTEP-M0 alive 1
 *   BACKSTEP-M0 alive 2
 *   ...
 *
 * SPDX-License-Identifier: MIT
 */
#include <stdint.h>

#include "backstep.h"
#include "vectors.h"

#define REG32(addr) (*(volatile uint32_t *)(addr))

#define RCC_BASE 0x40023800u
#define RCC_AHB1ENR REG32(RCC_BASE + 0x30u)
#define RCC_APB1ENR REG32(RCC_BASE + 0x40u)
#define RCC_AHB1ENR_GPIOAEN (1u << 0)
#define RCC_APB1ENR_USART2EN (1u << 17)

#define GPIOA_BASE 0x40020000u
#define GPIOA_MODER REG32(GPIOA_BASE + 0x00u)
#define GPIOA_ODR REG32(GPIOA_BASE + 0x14u)
#define GPIOA_AFRL REG32(GPIOA_BASE + 0x20u)

#define USART2_BASE 0x40004400u
#define USART2_SR REG32(USART2_BASE + 0x00u)
#define USART2_DR REG32(USART2_BASE + 0x04u)
#define USART2_BRR REG32(USART2_BASE + 0x08u)
#define USART2_CR1 REG32(USART2_BASE + 0x0Cu)
#define USART_SR_TXE (1u << 7)
#define USART_CR1_UE (1u << 13)
#define USART_CR1_TE (1u << 3)

#define SYST_CSR REG32(0xE000E010u)
#define SYST_RVR REG32(0xE000E014u)
#define SYST_CVR REG32(0xE000E018u)

#define HSI_HZ 16000000u
#define LED_PIN 5u

static volatile uint32_t g_ms;

void SysTick_Handler(void)
{
    g_ms++;
}

static void uart_putc(char c)
{
    while ((USART2_SR & USART_SR_TXE) == 0u) {
    }
    USART2_DR = (uint32_t)(unsigned char)c;
}

static void uart_puts(const char *s)
{
    while (*s != '\0') {
        uart_putc(*s++);
    }
}

static void uart_put_u32(uint32_t v)
{
    char buf[11];
    int i = 10;
    buf[i] = '\0';
    do {
        buf[--i] = (char)('0' + (v % 10u));
        v /= 10u;
    } while (v != 0u && i > 0);
    uart_puts(&buf[i]);
}

static void uart_put_hex32(uint32_t v)
{
    static const char hex[] = "0123456789ABCDEF";
    int shift;
    for (shift = 28; shift >= 0; shift -= 4) {
        uart_putc(hex[(v >> (unsigned)shift) & 0xFu]);
    }
}

int main(void)
{
    static const char crc_input[] = "123456789";
    uint32_t next_toggle = 500u;
    uint32_t next_beat = 1000u;
    uint32_t beats = 0u;

    RCC_AHB1ENR |= RCC_AHB1ENR_GPIOAEN;
    RCC_APB1ENR |= RCC_APB1ENR_USART2EN;
    (void)RCC_APB1ENR; /* read back: ensures the clock is on before use */

    /* PA5 = output (LED), PA2 = alternate function 7 (USART2_TX). */
    GPIOA_MODER = (GPIOA_MODER & ~((3u << (LED_PIN * 2u)) | (3u << 4))) |
                  (1u << (LED_PIN * 2u)) | (2u << 4);
    GPIOA_AFRL = (GPIOA_AFRL & ~(0xFu << 8)) | (7u << 8);

    /* 115200 baud from 16 MHz, oversampling 16: USARTDIV = 8.6875 -> 0x8B. */
    USART2_BRR = 0x8Bu;
    USART2_CR1 = USART_CR1_UE | USART_CR1_TE;

    /* 1 ms SysTick from the processor clock. */
    SYST_RVR = (HSI_HZ / 1000u) - 1u;
    SYST_CVR = 0u;
    SYST_CSR = 0x7u;

    uart_puts("\r\nBACKSTEP-M0 NUCLEO-F411RE blinky recorder=");
    uart_puts(BACKSTEP_VERSION_STRING);
    uart_puts(" crc=");
    uart_put_hex32(bs_crc32(crc_input, sizeof crc_input - 1u));
    uart_puts("\r\n");

    for (;;) {
        uint32_t now = g_ms;
        if ((int32_t)(now - next_toggle) >= 0) {
            GPIOA_ODR ^= (1u << LED_PIN);
            next_toggle += 500u;
        }
        if ((int32_t)(now - next_beat) >= 0) {
            beats++;
            uart_puts("BACKSTEP-M0 alive ");
            uart_put_u32(beats);
            uart_puts("\r\n");
            next_beat += 1000u;
        }
    }
}
