/*
 * Demo bug for reverse debugging (M5): an interrupt handler overruns a
 * buffer and corrupts a setting next to it; later, the main code
 * fails a check on that setting. Backstep records the run, and in GDB a
 * reverse watchpoint on the setting stops on the store that corrupted it.
 *
 * The "receive" interrupt (TIMER0) appends a byte to a 16-byte line
 * buffer. Its bound check is off by one, so the 17th byte of a line lands
 * in g_rx.limit, which follows the buffer. The main code takes a line
 * only now and then, so lines sometimes fill up. When it finds limit
 * changed, it reports the failure and the recording ends there (as a
 * crash dump would).
 *
 *   (gdb) continue                  -> end of the recording: in fail()
 *   (gdb) watch g_rx.limit
 *   (gdb) reverse-continue          -> rx_isr, at the overrunning store
 *
 * The recording goes to "recording.bsr".
 *
 * SPDX-License-Identifier: MIT
 */
#include <stdint.h>

#include "backstep.h"
#include "backstep_irq.h"
#include "backstep_record.h"
#include "qemu_test.h"
#include "vectors.h"

#define REG32(a) (*(volatile uint32_t *)(a))
#define NVIC_ISER0 0xE000E100u
#define NVIC_IPR(n) (*(volatile uint8_t *)(0xE000E400u + (n)))
#define TIMER0 0x40000000u
#define TIMER2 0x40002000u
#define T_CTRL 0x00u
#define T_VALUE 0x04u
#define T_RELOAD 0x08u
#define T_INTCLEAR 0x0Cu
#define TIMER0_IRQ 8u
#define LIMIT 100u

extern uint32_t _sdata, _ebss, _estack, __backstep_image_start, __backstep_image_end;

/* The received line, and a setting that happens to follow it. */
struct rx {
    char line[16];
    volatile uint32_t limit;
};
static struct rx g_rx = {{0}, LIMIT};
static volatile uint32_t g_rx_len;
static uint32_t g_lines, g_sum;

static struct bs_mmio_event g_log[4096] BS_STATE;
static struct bs_irq_event g_irqs[1024] BS_STATE;
static uint8_t g_ram[12288] BS_STATE;

void MemManage_Handler(void) __attribute__((naked));
void MemManage_Handler(void)
{
    __asm__ volatile("b bs_memmanage_handler");
}

void HardFault_Handler(void) __attribute__((naked));
void HardFault_Handler(void)
{
    __asm__ volatile("b bs_hardfault_handler");
}

void rx_isr(void);
void rx_stub(void);

/* A byte arrives (the timer's count stands in for a UART data register). */
void rx_isr(void)
{
    char c = (char)('a' + REG32(TIMER0 + T_VALUE) % 26u);
    REG32(TIMER0 + T_INTCLEAR) = 1u;
    if (g_rx_len <= sizeof g_rx.line) { /* BUG: should be < */
        g_rx.line[g_rx_len] = c;        /* DEMO-BUG-STORE */
        g_rx_len = g_rx_len + 1u;
    }
}

BS_IRQ_STUB(rx_stub, rx_isr)

__attribute__((section(".isr_vector.device"), used))
const void *const g_device_vectors[TIMER0_IRQ + 1u] = {
    [TIMER0_IRQ] = (const void *)rx_stub,
};

static uint32_t g_handle;
static uint32_t g_write_ok = 1u;

static void put(const void *data, uint32_t len, void *ctx)
{
    (void)ctx;
    g_write_ok &= semihost_write(g_handle, data, len) == 0;
}

static void write_recording(void)
{
    static const char k_name[] = "recording.bsr";
    uint32_t bytes;
    g_handle = semihost_open_write(k_name, sizeof k_name - 1u);
    bytes = bs_record_write(put, 0);
    semihost_close(g_handle);
    test_check(bytes != 0u && g_write_ok != 0u, "recording.bsr written");
}

/* The check that finds the damage, long after it was done. */
static void fail(void)
{
    bs_record_end();
    REG32(TIMER0 + T_CTRL) = 0u;
    write_recording();
    test_puts("demo: g_rx.limit was corrupted; recording written\n");
    test_finish();
}

/* Take the line, if the interrupt has filled some, then work a while. */
static void take_line(uint32_t round)
{
    uint32_t i, n;
    __asm__ volatile("cpsid i" ::: "memory");
    n = g_rx_len < sizeof g_rx.line ? g_rx_len : sizeof g_rx.line;
    for (i = 0; i < n; ++i) {
        g_sum = g_sum * 31u + (uint8_t)g_rx.line[i];
    }
    g_rx_len = 0u;
    __asm__ volatile("cpsie i" ::: "memory");
    g_lines++;
    /* Busy for longer and longer: sooner or later a line fills up. */
    for (i = 0; i < 1500u * (round % 8u); ++i) {
        g_sum = g_sum * 1664525u + 1013904223u;
    }
    g_sum ^= REG32(TIMER2 + T_VALUE); /* and a timer read now and then */
}

int main(void)
{
    struct bs_record_config cfg;
    uint32_t round = 0;

    test_uart_init();
    __asm__ volatile("cpsid i" ::: "memory");
    NVIC_IPR(TIMER0_IRQ) = 0x80u;
    REG32(TIMER2 + 0x00u) = 0xFFFFFFFFu; /* dual timer: free-running */
    REG32(TIMER2 + 0x08u) = 0x82u;
    REG32(TIMER0 + T_RELOAD) = 40u;
    REG32(TIMER0 + T_CTRL) = 1u | (1u << 3);
    REG32(NVIC_ISER0) = 1u << TIMER0_IRQ;

    cfg.target_id = BS_TARGET_QEMU_MPS2_AN386;
    cfg.image_start = &__backstep_image_start;
    cfg.image_end = &__backstep_image_end;
    cfg.ram_start = &_sdata;
    cfg.ram_end = &_ebss;
    cfg.stack_top = &_estack;
    cfg.stack_bottom = (const void *)((uintptr_t)&_estack - 8192u);
    cfg.log = g_log;
    cfg.log_capacity = sizeof g_log / sizeof g_log[0];
    cfg.ram_buffer = g_ram;
    cfg.ram_buffer_capacity = sizeof g_ram;
    cfg.irq_log = g_irqs;
    cfg.irq_capacity = sizeof g_irqs / sizeof g_irqs[0];
    __asm__ volatile("cpsie i" ::: "memory");

    if (bs_record_begin(&cfg) != 0) {
        test_check(0, "MPU present");
        test_finish();
    }
    /* ---- recorded ---- */
    while (round < 200u) {
        take_line(round++);
        if (g_rx.limit != LIMIT) {
            fail();
        }
    }
    bs_record_end();
    test_check(0, "the bug did not show (no corruption in 200 rounds)");
    test_finish();
    return 0;
}
