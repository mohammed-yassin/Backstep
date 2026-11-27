/*
 * M1 test: can an interrupt's exact position be recovered from the
 * recorded context (PC + registers + xPSR + peripheral-read count)?
 *
 * Varied workloads run while two interrupt sources fire at random
 * intervals (SysTick at priority 0x40 nests inside timer 0 at 0x80). Every
 * interrupt entry is recorded by the Backstep stub. At the end the events
 * are written to irq_events.bin through semihosting; QEMU's per-instruction
 * trace is the ground truth, and scripts/irq-position-check.py checks every
 * event against it.
 *
 * Command line (semihosting): "<seed> <events>", e.g. "3 2000".
 *
 * SPDX-License-Identifier: MIT
 */
#include <stdint.h>

#include "backstep.h"
#include "backstep_irq.h"
#include "qemu_test.h"
#include "vectors.h"

#define SYST_CSR QT_REG32(0xE000E010u)
#define SYST_RVR QT_REG32(0xE000E014u)
#define SYST_CVR QT_REG32(0xE000E018u)
#define SCB_SHPR3 QT_REG32(0xE000ED20u)
#define NVIC_ISER0 QT_REG32(0xE000E100u)
#define NVIC_ICER0 QT_REG32(0xE000E180u)
#define NVIC_IPR(n) (*(volatile uint8_t *)(0xE000E400u + (n)))

#define TIMER0 0x40000000u
#define TIMER1 0x40001000u
#define T_CTRL 0x00u
#define T_VALUE 0x04u
#define T_RELOAD 0x08u
#define T_INTCLEAR 0x0Cu
#define TIMER0_IRQ 8u

extern uint32_t _estack;

#define MAX_EVENTS 4000u

/* Unoptimised builds run handlers several times slower; space interrupts
 * further apart so the main code still runs between them. */
#if defined(__OPTIMIZE__)
#define SYSTICK_MIN 4u
#define SYSTICK_SPAN 8u
#define TIMER_MIN 8u
#define TIMER_SPAN 12u
#else
#define SYSTICK_MIN 14u
#define SYSTICK_SPAN 16u
#define TIMER_MIN 28u
#define TIMER_SPAN 24u
#endif
static struct bs_irq_event g_events[MAX_EVENTS];
volatile uint32_t g_mmio_reads; /* stands in for the MPU trap's counter */
static volatile uint32_t g_flag;
static uint32_t g_isr_rng = 0x9E3779B9u;
static uint32_t g_rng;
static uint32_t g_buf[32];
static uint32_t g_copy[32];
volatile uint32_t g_sink;

uint32_t mmio_read(uint32_t addr);

/* Every peripheral read goes through here so it can be counted. The
 * read and the count happen with interrupts masked, exactly as in the MPU
 * trap (where both happen inside the fault handler), so an interrupt can
 * never see one without the other. */
__attribute__((noinline)) uint32_t mmio_read(uint32_t addr)
{
    uint32_t v;
    __asm__ volatile("cpsid i" ::: "memory");
    g_mmio_reads++;
    v = *(volatile uint32_t *)addr;
    __asm__ volatile("cpsie i" ::: "memory");
    return v;
}

static uint32_t xorshift(uint32_t *s)
{
    uint32_t x = *s;
    x ^= x << 13;
    x ^= x >> 17;
    x ^= x << 5;
    *s = x;
    return x;
}

void systick_isr(void);
void timer0_isr(void);
void timer0_stub(void);

void systick_isr(void)
{
    uint32_t r = xorshift(&g_isr_rng);
    g_flag++;
    SYST_RVR = SYSTICK_MIN + (r % SYSTICK_SPAN); /* random next tick */
    SYST_CVR = 0u;
    if ((r & 3u) == 0u) {
        g_sink += mmio_read(TIMER1 + T_VALUE);
    }
}

void timer0_isr(void)
{
    uint32_t r = xorshift(&g_isr_rng);
    QT_REG32(TIMER0 + T_INTCLEAR) = 1u;
    QT_REG32(TIMER0 + T_RELOAD) = TIMER_MIN + (r % TIMER_SPAN);
    /* Long enough for SysTick to nest inside it now and then. */
    g_sink += bs_crc32(g_buf, 16u) + mmio_read(TIMER1 + T_VALUE);
}

BS_IRQ_STUB(SysTick_Handler, systick_isr)
BS_IRQ_STUB(timer0_stub, timer0_isr)

/* Device vectors IRQ0..IRQ8; only timer 0 (IRQ8) is used. */
__attribute__((section(".isr_vector.device"), used))
const void *const g_device_vectors[TIMER0_IRQ + 1u] = {
    [TIMER0_IRQ] = (const void *)timer0_stub,
};

static uint32_t fib(uint32_t n)
{
    return n < 2u ? n : fib(n - 1u) + fib(n - 2u);
}

static uint32_t f_add(uint32_t x) { return x + 0x1234u; }
static uint32_t f_mix(uint32_t x) { return (x << 3) ^ (x >> 5); }
static uint32_t f_mul(uint32_t x) { return x * 2654435761u; }

static void workload_step(void)
{
    static uint32_t (*const k_fns[3])(uint32_t) = {f_add, f_mix, f_mul};
    uint32_t i, j, r = xorshift(&g_rng);

    switch (r % 7u) {
    case 0: /* checksum */
        g_sink += bs_crc32(g_buf, 8u + (r >> 8) % 56u);
        break;
    case 1: { /* small bubble sort */
        uint32_t a[10];
        for (i = 0; i < 10u; ++i) {
            a[i] = xorshift(&g_rng) & 0xFFu;
        }
        for (i = 0; i < 10u; ++i) {
            for (j = 0; j + 1u < 10u - i; ++j) {
                if (a[j] > a[j + 1u]) {
                    uint32_t t = a[j];
                    a[j] = a[j + 1u];
                    a[j + 1u] = t;
                }
            }
        }
        g_sink += a[0] + a[9];
        break;
    }
    case 2: /* recursion */
        g_sink += fib(5u + (r >> 4) % 5u);
        break;
    case 3: /* peripheral polling */
        for (i = 0; i < 1u + (r >> 8) % 8u; ++i) {
            g_sink += mmio_read(TIMER1 + T_VALUE);
        }
        break;
    case 4: { /* spin on a RAM flag set by SysTick: registers do not change */
        uint32_t seen = g_flag;
        while (g_flag == seen) {
        }
        break;
    }
    case 5: /* memory copy */
        for (i = 0; i < 32u; ++i) {
            g_copy[i] = g_buf[(i + r) & 31u] ^ i;
        }
        break;
    default: /* indirect calls */
        g_sink = k_fns[r % 3u](g_sink);
        break;
    }
}

static uint32_t parse_u32(const char **p)
{
    uint32_t v = 0u;
    while (**p == ' ') {
        (*p)++;
    }
    while (**p >= '0' && **p <= '9') {
        v = v * 10u + (uint32_t)(**p - '0');
        (*p)++;
    }
    return v;
}

int main(void)
{
    char cmdline[48] = "1 2000";
    const char *p = cmdline;
    uint32_t seed, want, i;
    static const char k_out[] = "irq_events.bin";

    test_uart_init();
    (void)semihost_get_cmdline(cmdline, sizeof cmdline);
    seed = parse_u32(&p);
    want = parse_u32(&p);
    if (want == 0u || want > MAX_EVENTS) {
        want = 2000u;
    }
    g_rng = 0x12345678u ^ (seed * 0x9E3779B9u);
    g_isr_rng ^= seed * 0x85EBCA6Bu;
    for (i = 0; i < 32u; ++i) {
        g_buf[i] = xorshift(&g_rng);
    }

    bs_irq_record_start(g_events, MAX_EVENTS, &g_mmio_reads, &_estack);

    SCB_SHPR3 = (SCB_SHPR3 & 0x00FFFFFFu) | (0x40u << 24); /* SysTick */
    NVIC_IPR(TIMER0_IRQ) = 0x80u;
    QT_REG32(TIMER1 + T_RELOAD) = 0xFFFFFFFFu;
    QT_REG32(TIMER1 + T_CTRL) = 1u; /* free-running, no interrupt */
    QT_REG32(TIMER0 + T_RELOAD) = 5u;
    QT_REG32(TIMER0 + T_CTRL) = 1u | (1u << 3); /* enable + interrupt */
    NVIC_ISER0 = 1u << TIMER0_IRQ;
    SYST_RVR = 3u;
    SYST_CVR = 0u;
    SYST_CSR = 0x7u;

    while (bs_irq_record_count() < want) {
        workload_step();
    }

    SYST_CSR = 0u;
    NVIC_ICER0 = 1u << TIMER0_IRQ;
    QT_REG32(TIMER0 + T_CTRL) = 0u;
    want = bs_irq_record_count();
    if (want > MAX_EVENTS) {
        want = MAX_EVENTS;
    }
    test_check(semihost_write_file(k_out, sizeof k_out - 1u, g_events,
                                   want * (uint32_t)sizeof g_events[0]) == 0,
               "events written to irq_events.bin");
    test_finish();
    return 0;
}
