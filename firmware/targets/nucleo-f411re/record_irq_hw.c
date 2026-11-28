/*
 * M4 hardware test: record interrupt-driven code on the NUCLEO-F411RE and
 * replay it on the PC, 20 times, then once more ending in a crash.
 *
 * Three interrupt sources at three priorities fire at random rates and
 * nest: SysTick (0x40), TIM3 (0x80) and TIM4 (0xC0), all entering through
 * BS_IRQ_STUB so their positions are recorded. Each handler reads real
 * timer registers (recorded) and picks a random next period. The main
 * code runs a sort, checksums, recursion, polls the free-running TIM2 and
 * waits for flags the handlers set, until enough interrupts happened.
 *
 * (QEMU's netduinoplus2 runs this image in CI, but its STM32 timer model
 * schedules each period later the longer the machine has run, so there
 * TIM3 and TIM4 fire mostly in the first run; on a chip they fire in all.)
 *
 * Every run is recorded and sent over USART2 (the ST-LINK virtual COM
 * port) as hex, as record_hw.c does, preceded by its number:
 *
 *   BSREC RUN <n>
 *   BSREC BEGIN <bytes>
 *   <64 hex digits per line>
 *   BSREC END <CRC-32 of the file>
 *
 * Runs 1-20 end normally. In run 21 the SysTick handler corrupts a function
 * pointer after a while; the main code then faults (INVSTATE, escalated to
 * HardFault), and the fault ends the recording (a crash dump).
 * scripts/hil/m4_nucleo_f411re.py replays every run with `backstep
 * replay`. In CI the same image runs on QEMU's netduinoplus2
 * (scripts/qemu-nucleo-irq-smoke.py).
 *
 * SPDX-License-Identifier: MIT
 */
#include <stdint.h>

#include "backstep.h"
#include "backstep_irq.h"
#include "backstep_mmio.h"
#include "backstep_record.h"
#include "nucleo_test.h"
#include "vectors.h"

#define RUNS 20u
#define CRASH_RUN (RUNS + 1u)
#define INTERRUPTS_PER_RUN 150u

#define SYST_CSR NT_REG32(0xE000E010u)
#define SYST_RVR NT_REG32(0xE000E014u)
#define SYST_CVR NT_REG32(0xE000E018u)
#define SCB_SHPR3 NT_REG32(0xE000ED20u)
#define SCB_ICSR NT_REG32(0xE000ED04u)
#define NVIC_ISER0 NT_REG32(0xE000E100u)
#define NVIC_ICER0 NT_REG32(0xE000E180u)
#define NVIC_ICPR0 NT_REG32(0xE000E280u)
#define NVIC_IPR(n) (*(volatile uint8_t *)(0xE000E400u + (n)))

#define TIM2 0x40000000u /* 32-bit, free-running: polled by the main code */
#define TIM3 0x40000400u
#define TIM4 0x40000800u
#define T_CR1 0x00u
#define T_DIER 0x0Cu
#define T_SR 0x10u
#define T_EGR 0x14u
#define T_CNT 0x24u
#define T_PSC 0x28u
#define T_ARR 0x2Cu
#define TIM(base, reg) NT_REG32((base) + (reg))
#define TIM3_IRQ 29u
#define TIM4_IRQ 30u

extern uint32_t _sdata, _ebss, _estack, __backstep_image_start, __backstep_image_end;

static uint32_t g_hash = 0x811C9DC5u;
static uint32_t g_rng = 1u;
static volatile uint32_t g_interrupts;
static volatile uint32_t g_flags[3];
static volatile uint32_t g_shared[16];
static uint32_t g_buf[48];
static uint32_t g_run;
static uint32_t g_complete; /* runs recorded and sent */
static void (*volatile g_callback)(void);

static struct bs_mmio_event g_log[2048] BS_STATE;
static struct bs_irq_event g_irqs[256] BS_STATE;
static uint8_t g_ram[8192] BS_STATE;

__attribute__((naked)) void MemManage_Handler(void)
{
    __asm__ volatile("b bs_memmanage_handler");
}

__attribute__((naked)) void HardFault_Handler(void)
{
    __asm__ volatile("b bs_hardfault_handler");
}

static uint32_t next_random(void)
{
    g_rng ^= g_rng << 13;
    g_rng ^= g_rng >> 17;
    g_rng ^= g_rng << 5;
    return g_rng;
}

static void fold(uint32_t v)
{
    g_hash = (g_hash ^ v) * 0x01000193u;
}

static void callback_ok(void)
{
    fold(0x600Du);
}

void systick_isr(void);
void tim3_isr(void);
void tim4_isr(void);
void systick_stub(void);
void tim3_stub(void);
void tim4_stub(void);

/* At 16 MHz (HSI, the reset clock) the periods below are 0.2-0.9 ms for
 * SysTick and 0.1-0.4 ms for the timers: the handlers, slowed by the
 * capture of their timer reads, nest often. */
void systick_isr(void)
{
    uint32_t r = next_random();
    SYST_RVR = 3000u + (r % 12000u); /* next tick: random (a PPB store) */
    SYST_CVR = 0u;
    g_shared[r & 15u] += TIM(TIM2, T_CNT);
    g_flags[0]++;
    g_interrupts++;
    if (g_run == CRASH_RUN && g_interrupts > INTERRUPTS_PER_RUN / 2u) {
        /* Bit 0 clear: branching there leaves Thumb state (INVSTATE). */
        g_callback = (void (*)(void))0x08001000u;
    }
}

void tim3_isr(void)
{
    uint32_t r = next_random();
    TIM(TIM3, T_SR) = 0u; /* clear UIF */
    TIM(TIM3, T_ARR) = 1500u + (r % 4500u);
    g_shared[(r >> 4) & 15u] ^= TIM(TIM3, T_CNT) + bs_crc32((const void *)g_shared, 16u);
    fold(TIM(TIM3, T_SR)); /* also makes sure the flag is clear before returning */
    g_flags[1]++;
    g_interrupts++;
}

void tim4_isr(void)
{
    uint32_t r = next_random(), i, acc = 0;
    TIM(TIM4, T_SR) = 0u;
    TIM(TIM4, T_ARR) = 2000u + (r % 6000u);
    for (i = 0; i < 24u; ++i) { /* long enough to be preempted */
        acc = acc * 31u + g_shared[i & 15u];
    }
    g_shared[r & 15u] = acc ^ TIM(TIM4, T_CNT);
    fold(TIM(TIM4, T_SR));
    g_flags[2]++;
    g_interrupts++;
}

BS_IRQ_STUB(systick_stub, systick_isr)
BS_IRQ_STUB(tim3_stub, tim3_isr)
BS_IRQ_STUB(tim4_stub, tim4_isr)

__attribute__((naked)) void SysTick_Handler(void)
{
    __asm__ volatile("b systick_stub");
}

__attribute__((section(".isr_vector.device"), used))
const void *const g_device_vectors[TIM4_IRQ + 1u] = {
    [TIM3_IRQ] = (const void *)tim3_stub,
    [TIM4_IRQ] = (const void *)tim4_stub,
};

static uint32_t deep(uint32_t depth)
{
    uint32_t local[3], i, acc = 0;
    for (i = 0; i < 3u; ++i) {
        local[i] = g_shared[(depth + i) & 15u] + depth;
    }
    if (depth > 0u) {
        acc = deep(depth - 1u);
    }
    for (i = 0; i < 3u; ++i) {
        acc = acc * 7u + local[i];
    }
    return acc;
}

static void wait_flag(unsigned which)
{
    uint32_t start = g_flags[which], n = 0;
    while (g_flags[which] == start && n < 200000u) { /* spin on a RAM flag */
        ++n;
    }
    fold(n);
}

static void round_of_work(uint32_t round)
{
    uint32_t i, j;
    for (i = 0; i < 48u; ++i) {
        g_buf[i] = next_random() ^ g_shared[i & 15u];
    }
    for (i = 1; i < 24u; ++i) { /* insertion sort of half the buffer */
        uint32_t v = g_buf[i];
        for (j = i; j > 0u && g_buf[j - 1u] > v; --j) {
            g_buf[j] = g_buf[j - 1u];
        }
        g_buf[j] = v;
    }
    fold(bs_crc32(g_buf, sizeof g_buf));
    fold(deep(4u + (round & 3u)));
    for (i = 0; i < 4u; ++i) {
        fold(TIM(TIM2, T_CNT)); /* polling from the main code */
    }
    wait_flag(round % 3u);
    g_callback();
}

static void timers_start(void)
{
    uint32_t r = next_random();
    /* Enable (URS: only overflows interrupt), restart from 0 (UG), then
     * set the period: QEMU's timer model arms itself on that write. */
    TIM(TIM3, T_CR1) = 1u | (1u << 2);
    TIM(TIM4, T_CR1) = 1u | (1u << 2);
    TIM(TIM3, T_EGR) = 1u;
    TIM(TIM4, T_EGR) = 1u;
    TIM(TIM3, T_ARR) = 1500u + (r % 4500u);
    TIM(TIM4, T_ARR) = 2000u + ((r >> 12) % 6000u);
    TIM(TIM3, T_SR) = 0u;
    TIM(TIM4, T_SR) = 0u;
    NVIC_ICPR0 = (1u << TIM3_IRQ) | (1u << TIM4_IRQ);
    NVIC_ISER0 = (1u << TIM3_IRQ) | (1u << TIM4_IRQ);
    SYST_RVR = 5000u + ((r >> 20) % 4000u);
    SYST_CVR = 0u;
    SYST_CSR = 7u; /* processor clock, interrupt, enable */
}

static void timers_stop(void)
{
    SYST_CSR = 0u;
    NVIC_ICER0 = (1u << TIM3_IRQ) | (1u << TIM4_IRQ);
    TIM(TIM3, T_CR1) = 0u;
    TIM(TIM4, T_CR1) = 0u;
    TIM(TIM3, T_SR) = 0u;
    TIM(TIM4, T_SR) = 0u;
    NVIC_ICPR0 = (1u << TIM3_IRQ) | (1u << TIM4_IRQ);
    SCB_ICSR = 1u << 25; /* PENDSTCLR */
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

static void crc_piece(const void *data, uint32_t len, void *ctx)
{
    uint32_t *crc = (uint32_t *)ctx;
    *crc = bs_crc32_update(*crc, data, len);
}

/* Send the finished recording; returns 0 if there was none. */
static uint32_t send_recording(void)
{
    uint32_t crc = 0u, column = 0u, bytes;

    timers_stop();
    bytes = bs_record_write(crc_piece, &crc);
    test_puts("BSREC RUN ");
    test_put_u32(g_run);
    test_puts("\n");
    if (bytes == 0u) {
        /* Unusable: a log overflowed (or the recorder failed). */
        test_puts("BSREC NONE loads ");
        test_put_u32(bs_mmio_trap_stats()->loads);
        test_puts(" dropped ");
        test_put_u32(bs_mmio_trap_stats()->dropped);
        test_puts(" interrupts ");
        test_put_u32(bs_irq_record_count());
        test_puts("\n");
        return 0u;
    }
    test_puts("BSREC BEGIN ");
    test_put_u32(bytes);
    test_puts("\n");
    (void)bs_record_write(dump_hex, &column);
    if (column != 0u) {
        test_putc('\n');
    }
    test_puts("BSREC END ");
    test_put_hex(crc);
    test_puts("\n");
    return bytes;
}

/* The crash run's genuine fault: end the recording here, in HardFault. */
void bs_mmio_trap_fatal(uint32_t pc, uint32_t reason)
{
    bs_record_end();
    test_puts("crash at ");
    test_put_hex(pc);
    test_puts(" reason ");
    test_put_u32(reason);
    test_puts("\n");
    test_check(g_complete == RUNS, "20 recordings complete");
    test_check(g_run == CRASH_RUN && send_recording() != 0u, "crash recording complete");
    test_finish();
    for (;;) {
    }
}

int main(void)
{
    struct bs_record_config cfg;
    uint32_t round = 0u;

    test_uart_init();
    NT_RCC_APB1ENR |= (1u << 0) | (1u << 1) | (1u << 2); /* TIM2, TIM3, TIM4 */
    (void)NT_RCC_APB1ENR;
    TIM(TIM2, T_PSC) = 0u;
    TIM(TIM2, T_ARR) = 0xFFFFFFFFu;
    TIM(TIM2, T_EGR) = 1u;
    TIM(TIM2, T_CR1) = 1u; /* count at the timer clock */
    TIM(TIM3, T_PSC) = 0u;
    TIM(TIM4, T_PSC) = 0u;
    TIM(TIM3, T_DIER) = 1u; /* update interrupt */
    TIM(TIM4, T_DIER) = 1u;
    SCB_SHPR3 = (SCB_SHPR3 & 0x00FFFFFFu) | (0x40u << 24); /* SysTick */
    NVIC_IPR(TIM3_IRQ) = 0x80u;
    NVIC_IPR(TIM4_IRQ) = 0xC0u;
    g_callback = callback_ok;
    test_puts("BACKSTEP-M4 NUCLEO-F411RE interrupt record and replay test\n");

    cfg.target_id = BS_TARGET_NUCLEO_F411RE;
    cfg.image_start = &__backstep_image_start;
    cfg.image_end = &__backstep_image_end;
    cfg.ram_start = &_sdata;
    cfg.ram_end = &_ebss;
    cfg.stack_top = &_estack;
    cfg.stack_bottom = (const void *)((uintptr_t)&_estack - 4096u); /* the stack area */
    cfg.log = g_log;
    cfg.log_capacity = sizeof g_log / sizeof g_log[0];
    cfg.ram_buffer = g_ram;
    cfg.ram_buffer_capacity = sizeof g_ram;
    cfg.irq_log = g_irqs;
    cfg.irq_capacity = sizeof g_irqs / sizeof g_irqs[0];

    for (g_run = 1u; g_run <= CRASH_RUN; ++g_run) {
        /* A different run every time: the free-running TIM2 differs on a
         * chip from run to run. */
        g_rng = (TIM(TIM2, T_CNT) ^ (g_run * 0x9E3779B1u)) | 1u;
        g_interrupts = 0u;
        __asm__ volatile("cpsid i" ::: "memory");
        timers_start();
        __asm__ volatile("cpsie i" ::: "memory");
        if (bs_record_begin(&cfg) != 0) {
            test_check(0, "MPU present");
            test_finish();
        }
        /* ---- recorded ---- */
        while (g_interrupts < INTERRUPTS_PER_RUN) {
            round_of_work(round++);
        }
        bs_record_end();
        /* ---- not recorded ---- */
        g_complete += send_recording() != 0u;
    }
    /* Only reached if the crash run did not crash. */
    test_check(g_complete == RUNS, "20 recordings complete");
    test_check(0, "crash recording complete");
    test_finish();
    return 0;
}
