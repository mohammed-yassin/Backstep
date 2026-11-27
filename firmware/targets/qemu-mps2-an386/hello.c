/*
 * M0 self-test firmware for QEMU mps2-an386.
 *
 * Checks that the shared startup code and the toolchain work end to end:
 *   1. .data is copied from flash and .bss is zeroed,
 *   2. the CMSDK UART prints text (QEMU maps it to stdout),
 *   3. SysTick interrupts fire through the vector table,
 *   4. the recorder's CRC-32 gives the standard check value.
 * Result is reported through the QEMU exit code via semihosting.
 *
 * SPDX-License-Identifier: MIT
 */
#include <stdint.h>

#include "backstep.h"
#include "vectors.h"
#include "semihost.h"

#define UART0_BASE 0x40004000u
#define UART0_DATA (*(volatile uint32_t *)(UART0_BASE + 0x00u))
#define UART0_STATE (*(volatile uint32_t *)(UART0_BASE + 0x04u))
#define UART0_CTRL (*(volatile uint32_t *)(UART0_BASE + 0x08u))
#define UART0_BAUDDIV (*(volatile uint32_t *)(UART0_BASE + 0x10u))
#define UART_STATE_TXFULL 0x1u
#define UART_CTRL_TXEN 0x1u

#define SYST_CSR (*(volatile uint32_t *)0xE000E010u)
#define SYST_RVR (*(volatile uint32_t *)0xE000E014u)
#define SYST_CVR (*(volatile uint32_t *)0xE000E018u)

static volatile uint32_t g_ticks;            /* .bss: must start at 0 */
static volatile uint32_t g_marker = 0xB5EEDu; /* .data: must keep its value */

void SysTick_Handler(void)
{
    g_ticks++;
}

static void uart_putc(char c)
{
    while (UART0_STATE & UART_STATE_TXFULL) {
    }
    UART0_DATA = (uint32_t)(unsigned char)c;
}

static void uart_puts(const char *s)
{
    while (*s != '\0') {
        uart_putc(*s++);
    }
}

static void check(int ok, const char *name, int *failures)
{
    uart_puts(ok ? "PASS " : "FAIL ");
    uart_puts(name);
    uart_puts("\n");
    if (!ok) {
        (*failures)++;
    }
}

int main(void)
{
    int failures = 0;
    static const char crc_input[] = "123456789";

    UART0_BAUDDIV = 16u;
    UART0_CTRL = UART_CTRL_TXEN;

    uart_puts("Backstep M0 QEMU self-test (recorder ");
    uart_puts(BACKSTEP_VERSION_STRING);
    uart_puts(")\n");

    check(g_marker == 0xB5EEDu, "data-section-copied", &failures);
    check(g_ticks == 0u, "bss-section-zeroed", &failures);
    check(bs_crc32(crc_input, sizeof crc_input - 1u) == 0xCBF43926u,
          "recorder-crc32-check-value", &failures);

    /* SysTick from the processor clock; the exact rate does not matter. */
    SYST_RVR = 10000u - 1u;
    SYST_CVR = 0u;
    SYST_CSR = 0x7u; /* ENABLE | TICKINT | CLKSOURCE=processor */

    {
        uint32_t spins = 0u;
        while (g_ticks < 3u && spins < 50000000u) {
            spins++;
        }
    }
    SYST_CSR = 0u;
    check(g_ticks >= 3u, "systick-interrupts-fire", &failures);

    uart_puts(failures == 0 ? "RESULT PASS\n" : "RESULT FAIL\n");
    semihost_exit(failures == 0);
    return 0;
}
