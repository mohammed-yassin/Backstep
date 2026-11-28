/*
 * M3 test: record a polling program on QEMU, then replay it with
 * `backstep replay` (scripts/replay-test.sh).
 *
 * The semihosting command line selects one of ten workloads ("w=N"). Each
 * reads peripherals whose values QEMU does not repeat between runs (free-
 * running timers and counters, the UART status), makes control-flow
 * decisions on them, and folds every value read into g_hash (also the ones
 * that only decide when a loop ends), so a replay that got any value wrong
 * ends with different RAM. The recording goes to
 * the host file "recording.bsr".
 *
 * SPDX-License-Identifier: MIT
 */
#include <stdint.h>

#include "backstep.h"
#include "backstep_record.h"
#include "qemu_test.h"
#include "vectors.h"

#define REG32(a) (*(volatile uint32_t *)(a))
#define REG16(a) (*(volatile uint16_t *)(a))
#define REG8(a) (*(volatile uint8_t *)(a))
#define S16(a) (*(volatile int16_t *)(a))
#define S8(a) (*(volatile int8_t *)(a))

#define TIMER0 0x40000000u /* CMSDK timer: CTRL 0, VALUE 4, RELOAD 8 */
#define TIMER1 0x40001000u
#define DUALTIMER 0x40002000u /* Timer1Load 0, Timer1Value 4, Timer1Control 8 */
#define UART0_STATE 0x40004004u
#define FPGAIO 0x40028000u /* LED0 0, BUTTON 8, CLK1HZ 0x10, CLK100HZ 0x14, COUNTER 0x18 */

extern uint32_t _sdata, _ebss, _estack, __backstep_image_start, __backstep_image_end;

static uint32_t g_hash = 0x811C9DC5u; /* every peripheral value ends up here */
static uint32_t g_workload;
static uint32_t g_array[48];
static uint32_t g_scratch[16];

static struct bs_mmio_event g_log[4096] __attribute__((section(".backstep")));
static uint8_t g_ram[16384] __attribute__((section(".backstep")));

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

static void fold(uint32_t v)
{
    g_hash = (g_hash ^ v) * 0x01000193u;
}

static uint32_t timer_start(uint32_t base)
{
    uint32_t v;
    REG32(base + 8u) = 0xFFFFFFFFu; /* RELOAD */
    REG32(base + 0u) = 1u;          /* CTRL: enable */
    v = REG32(base + 4u);
    fold(v); /* every value read counts, even when the caller ignores it */
    return v;
}

/* 0: print through the polled UART (every STATE read is recorded). */
static void w_uart(void)
{
    test_puts("recorded and replayed: hello from the polled UART\n");
    fold(REG32(UART0_STATE));
}

/* 1: busy-wait on a free-running timer; the iteration count varies. */
static void w_timer_wait(void)
{
    uint32_t start = timer_start(TIMER0), n = 0, v;
    while (v = REG32(TIMER0 + 4u), fold(v), start - v < 50000u && n < 100000u) {
        ++n;
    }
    fold(n);
    fold(REG32(TIMER0 + 4u));
}

/* 2: a counter as the seed of a sort: data-dependent control flow. */
static void w_seeded_sort(void)
{
    uint32_t x = REG32(FPGAIO + 0x18u) | 1u, i, j;
    fold(x);
    for (i = 0; i < 48u; ++i) {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        g_array[i] = x;
    }
    for (i = 1; i < 48u; ++i) {
        uint32_t v = g_array[i];
        for (j = i; j > 0u && g_array[j - 1u] > v; --j) {
            g_array[j] = g_array[j - 1u];
        }
        g_array[j] = v;
    }
    fold(g_array[0] ^ g_array[47]);
}

/* 3: byte, halfword and signed loads of peripheral registers. */
static void w_sizes(void)
{
    uint32_t i;
    timer_start(TIMER1);
    for (i = 0; i < 20u; ++i) {
        fold(REG8(TIMER1 + 4u));
        fold(REG16(TIMER1 + 4u));
        fold((uint32_t)(int32_t)S8(FPGAIO + 0x18u));
        fold((uint32_t)(int32_t)S16(FPGAIO + 0x1Au));
        fold(REG8(TIMER1 + 5u) + REG16(TIMER1 + 6u));
    }
}

/* 4: branches and calls chosen by counter bits. */
static uint32_t pick(uint32_t v, int depth)
{
    switch (v & 3u) {
    case 0: return v * 3u;
    case 1: return depth > 0 ? pick(v >> 2, depth - 1) + 1u : v;
    case 2: return v ^ REG32(FPGAIO + 0x18u);
    default: return ~v;
    }
}

static void w_branchy(void)
{
    uint32_t i;
    timer_start(TIMER0);
    for (i = 0; i < 64u; ++i) {
        fold(pick(REG32(TIMER0 + 4u) ^ i, 6));
    }
}

/* 5: the dual timer. */
static void w_dualtimer(void)
{
    uint32_t i, last;
    REG32(DUALTIMER + 0x00u) = 0x00FFFFFFu;
    REG32(DUALTIMER + 0x08u) = 0x82u; /* enable, 32-bit */
    last = REG32(DUALTIMER + 0x04u);
    for (i = 0; i < 30u; ++i) {
        uint32_t v = REG32(DUALTIMER + 0x04u);
        fold(last - v);
        last = v;
    }
}

/* 6: write registers and read them back (stores are emulated by the
 * capture, loads recorded), plus constant registers. */
static void w_readback(void)
{
    uint32_t i;
    for (i = 0; i < 8u; ++i) {
        REG32(FPGAIO + 0x00u) = i & 3u; /* LED0 */
        fold(REG32(FPGAIO + 0x00u));
        REG32(TIMER1 + 8u) = 0x1000u + i; /* RELOAD */
        fold(REG32(TIMER1 + 8u));
    }
    fold(REG32(FPGAIO + 0x08u)); /* BUTTON */
}

/* 7: slow clocks polled until they change or a bound is reached. */
static void w_slow_clocks(void)
{
    uint32_t c100 = REG32(FPGAIO + 0x14u), n = 0, v;
    fold(c100);
    while (v = REG32(FPGAIO + 0x14u), fold(v), v == c100 && n < 20000u) {
        ++n;
    }
    fold(n);
    fold(REG32(FPGAIO + 0x10u));
    fold(REG32(FPGAIO + 0x14u));
}

/* 8: peripheral values kept on the stack through recursion. */
static uint32_t deep(uint32_t depth)
{
    uint32_t local[4], i, acc = 0;
    for (i = 0; i < 4u; ++i) {
        local[i] = REG32(TIMER0 + 4u) + depth;
    }
    if (depth > 0u) {
        acc = deep(depth - 1u);
    }
    for (i = 0; i < 4u; ++i) {
        acc = acc * 7u + local[i];
    }
    return acc;
}

static void w_recursion(void)
{
    timer_start(TIMER0);
    fold(deep(10u));
}

/* 9: conditional loads in IT blocks, writeback and register offsets. */
static void w_forms(void)
{
    uint32_t i, a = 0, b = 0;
    uint32_t base = FPGAIO;
    for (i = 0; i < 16u; ++i) {
        __asm__ volatile(
            "ldr %[a], [%[base], #0x18]\n\t" /* COUNTER */
            "tst %[a], #1\n\t"
            "ite ne\n\t"
            "ldrne %[b], [%[base], #0x18]\n\t"
            "ldreq %[b], [%[base], #0x14]\n\t"
            : [a] "=&r"(a), [b] "=&r"(b)
            : [base] "r"(base)
            : "cc", "memory");
        fold(a);
        fold(b);
    }
    {
        uint32_t p = TIMER0 + 4u, v, off = 4u;
        timer_start(TIMER0);
        __asm__ volatile("ldr %[v], [%[p]], #0\n\t" : [v] "=r"(v), [p] "+r"(p) : : "memory");
        fold(v);
        __asm__ volatile("ldr %[v], [%[q], %[o]]\n\t"
                         : [v] "=r"(v)
                         : [q] "r"(TIMER0), [o] "r"(off)
                         : "memory");
        fold(v);
    }
    for (i = 0; i < 16u; ++i) {
        g_scratch[i] = g_hash + i;
    }
}

static void (*const k_workloads[10])(void) = {
    w_uart, w_timer_wait, w_seeded_sort, w_sizes, w_branchy,
    w_dualtimer, w_readback, w_slow_clocks, w_recursion, w_forms,
};

static uint32_t g_handle;
static uint32_t g_write_ok;

static void put(const void *data, uint32_t len, void *ctx)
{
    (void)ctx;
    g_write_ok &= semihost_write(g_handle, data, len) == 0;
}

int main(void)
{
    static const char k_name[] = "recording.bsr";
    char cmdline[16] = {0};
    struct bs_record_config cfg;
    uint32_t bytes;

    test_uart_init();
    if (semihost_get_cmdline(cmdline, sizeof cmdline) == 0 && cmdline[0] == 'w' &&
        cmdline[1] == '=' && cmdline[2] >= '0' && cmdline[2] <= '9') {
        g_workload = (uint32_t)(cmdline[2] - '0');
    }
    __asm__ volatile("cpsid i" ::: "memory"); /* polling only: no interrupts (M4) */

    cfg.target_id = BS_TARGET_QEMU_MPS2_AN386;
    cfg.image_start = &__backstep_image_start;
    cfg.image_end = &__backstep_image_end;
    cfg.ram_start = &_sdata;
    cfg.ram_end = &_ebss;
    cfg.stack_top = &_estack;
    cfg.log = g_log;
    cfg.log_capacity = 4096u;
    cfg.ram_buffer = g_ram;
    cfg.ram_buffer_capacity = sizeof g_ram;

    if (bs_record_begin(&cfg) != 0) {
        test_check(0, "MPU present");
        test_finish();
    }
    /* ---- recorded ---- */
    k_workloads[g_workload]();
    bs_record_end();
    /* ---- not recorded ---- */

    g_handle = semihost_open_write(k_name, sizeof k_name - 1u);
    g_write_ok = g_handle != 0xFFFFFFFFu;
    bytes = bs_record_write(put, 0);
    semihost_close(g_handle);
    test_check(bytes != 0u && g_write_ok != 0u, "recording.bsr written");
    test_puts("workload ");
    test_put_u32(g_workload);
    test_puts(": hash ");
    test_put_hex(g_hash);
    test_puts(", recording ");
    test_put_u32(bytes);
    test_puts(" bytes\n");
    test_finish();
    return 0;
}
