/*
 * M3 hardware test: record a polling program on the NUCLEO-F411RE and
 * replay it on the PC.
 *
 * The board records several workloads that read real STM32F411 registers:
 * the free-running TIM2 counter (16 MHz, a different value every run), the
 * GPIO input registers, the USART status while printing, and RCC. Every
 * value read is folded into g_hash. After the recording it sends the
 * recording file over USART2 (the ST-LINK virtual COM port) as hex:
 *
 *   BSREC BEGIN <bytes>
 *   <64 hex digits per line>
 *   BSREC END <CRC-32 of the file, 8 hex digits>
 *
 * scripts/hil/m3_nucleo_f411re.py turns that back into recording.bsr and
 * runs `backstep replay`, which must reproduce the end registers and RAM
 * checksum the board recorded. The same image runs in CI on QEMU's
 * netduinoplus2 (STM32F405): scripts/qemu-nucleo-replay-smoke.sh.
 *
 * SPDX-License-Identifier: MIT
 */
#include <stdint.h>

#include "backstep.h"
#include "backstep_record.h"
#include "nucleo_test.h"
#include "vectors.h"

#define RCC_CR NT_REG32(0x40023800u)
#define RCC_CFGR NT_REG32(0x40023808u)
#define TIM2_CR1 NT_REG32(0x40000000u)
#define TIM2_CNT NT_REG32(0x40000024u)
#define TIM2_PSC NT_REG32(0x40000028u)
#define TIM2_ARR NT_REG32(0x4000002Cu)
#define TIM2_EGR NT_REG32(0x40000014u)
#define GPIOA_IDR NT_REG32(0x40020010u)
#define GPIOC_IDR NT_REG32(0x40020810u) /* PC13: the blue user button */
#define GPIOC_IDR16 (*(volatile uint16_t *)0x40020810u)
#define TIM2_CNT8 (*(volatile uint8_t *)0x40000024u)

extern uint32_t _sdata, _ebss, _estack, __backstep_image_start, __backstep_image_end;

static uint32_t g_hash = 0x811C9DC5u;
static uint32_t g_sorted[32];

static struct bs_mmio_event g_log[2048] BS_STATE;
static uint8_t g_ram[8192] BS_STATE;

__attribute__((naked)) void MemManage_Handler(void)
{
    __asm__ volatile("b bs_memmanage_handler");
}

__attribute__((naked)) void HardFault_Handler(void)
{
    __asm__ volatile("b bs_hardfault_handler");
}

static void fold(uint32_t v)
{
    g_hash = (g_hash ^ v) * 0x01000193u;
}

/* Busy-wait on TIM2; the number of polls depends on the run. */
static void w_timer(void)
{
    uint32_t start = TIM2_CNT, v, n = 0;
    fold(start);
    while (v = TIM2_CNT, fold(v), v - start < 2000u && n < 10000u) {
        ++n;
    }
    fold(n);
}

/* A sort seeded by the counter: data-dependent control flow. */
static void w_sort(void)
{
    uint32_t x = TIM2_CNT | 1u, i, j;
    fold(x);
    for (i = 0; i < 32u; ++i) {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        g_sorted[i] = x;
    }
    for (i = 1; i < 32u; ++i) {
        uint32_t v = g_sorted[i];
        for (j = i; j > 0u && g_sorted[j - 1u] > v; --j) {
            g_sorted[j] = g_sorted[j - 1u];
        }
        g_sorted[j] = v;
    }
    fold(g_sorted[0] ^ g_sorted[31]);
}

/* Inputs and sub-word loads. */
static void w_inputs(void)
{
    uint32_t i;
    for (i = 0; i < 8u; ++i) {
        fold(GPIOC_IDR);
        fold(GPIOA_IDR);
        fold(GPIOC_IDR16);
        fold(TIM2_CNT8);
        if (GPIOC_IDR & (1u << 13)) { /* button not pressed (pulled up) */
            fold(RCC_CR);
        } else {
            fold(RCC_CFGR);
        }
    }
}

/* Printing through the polled USART: every status read is recorded. */
static void w_print(void)
{
    test_puts("BSREC recording: printed while recorded\n");
}

static void dump_hex(const void *data, uint32_t len, void *ctx)
{
    static const char hex[] = "0123456789ABCDEF";
    uint32_t *column = (uint32_t *)ctx;
    const uint8_t *p = (const uint8_t *)data;
    uint32_t i;
    for (i = 0; i < len; ++i) {
        test_putc(hex[p[i] >> 4]);
        test_putc(hex[p[i] & 0xFu]);
        if (++*column == 32u) {
            test_putc('\n');
            *column = 0u;
        }
    }
}

struct crc_ctx {
    uint32_t crc;
};

static void crc_piece(const void *data, uint32_t len, void *ctx)
{
    struct crc_ctx *c = (struct crc_ctx *)ctx;
    c->crc = bs_crc32_update(c->crc, data, len);
}

int main(void)
{
    struct bs_record_config cfg;
    struct crc_ctx crc = {0u};
    uint32_t column = 0u, bytes;

    test_uart_init();
    NT_RCC_APB1ENR |= 1u << 0; /* TIM2 */
    (void)NT_RCC_APB1ENR;
    TIM2_PSC = 0u;
    TIM2_ARR = 0xFFFFFFFFu;
    TIM2_EGR = 1u;
    TIM2_CR1 = 1u; /* count at the timer clock */
    test_puts("BACKSTEP-M3 NUCLEO-F411RE record and replay test\n");
    __asm__ volatile("cpsid i" ::: "memory"); /* polling only (M4 adds interrupts) */

    cfg.target_id = BS_TARGET_NUCLEO_F411RE;
    cfg.image_start = &__backstep_image_start;
    cfg.image_end = &__backstep_image_end;
    cfg.ram_start = &_sdata;
    cfg.ram_end = &_ebss;
    cfg.stack_top = &_estack;
    cfg.log = g_log;
    cfg.log_capacity = 2048u;
    cfg.ram_buffer = g_ram;
    cfg.ram_buffer_capacity = sizeof g_ram;

    if (bs_record_begin(&cfg) != 0) {
        test_check(0, "MPU present");
        test_finish();
    }
    /* ---- recorded ---- */
    w_timer();
    w_sort();
    w_inputs();
    w_print();
    w_timer();
    bs_record_end();
    /* ---- not recorded ---- */

    bytes = bs_record_write(crc_piece, &crc);
    test_check(bytes != 0u, "recording complete");
    test_puts("BSREC BEGIN ");
    test_put_u32(bytes);
    test_puts("\n");
    (void)bs_record_write(dump_hex, &column);
    if (column != 0u) {
        test_putc('\n');
    }
    test_puts("BSREC END ");
    test_put_hex(crc.crc);
    test_puts("\n");
    test_finish();
    return 0;
}
