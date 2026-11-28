/*
 * M4 test: record a program with nested interrupts from three sources at
 * random rates on QEMU, then replay it (scripts/replay-irq-test.py).
 *
 * Interrupt sources, from most to least urgent:
 *   SysTick  (priority 0x40), re-armed with a random period
 *   timer 0  (IRQ 8, 0x80),   random reload
 *   timer 1  (IRQ 9, 0xC0),   random reload
 * Each handler reads a timer (a recorded peripheral load, from inside the
 * handler), updates shared state and sets a flag the main code waits on.
 * The main code runs checksums, a sort, recursion, peripheral polling and
 * waits for those flags, until enough interrupts have happened.
 *
 * Command line (semihosting): "s=<seed>[:n=<interrupts>]" chooses the
 * random rates and how many interrupts to record (default 400);
 * "c=<seed>[:n=<interrupts>]" makes the timer 1 handler corrupt a function
 * pointer after 3/8 of them, so the main code faults: the HardFault ends
 * the recording (a crash dump). "a=<seed>[:n=<interrupts>]" adds a wait
 * loop whose iterations the interrupt key cannot tell apart (ADR 0004):
 * replay must find the right one (ADR 0009, lookahead).
 * The recording goes to "recording.bsr".
 *
 * Built with BS_TEST_FPU (replay_irq_test_fp.elf, hard float, M7), the
 * FPU is enabled before recording, the main code does floating-point work
 * between the integer work, and the SysTick and timer 1 handlers do too
 * (timer 0's does not): interrupts then push extended frames, lazily, the
 * handlers' first FP instruction preserves the main code's registers, and
 * MPU traps happen with a floating-point context active.
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
#define SYST_CSR 0xE000E010u
#define SYST_RVR 0xE000E014u
#define SYST_CVR 0xE000E018u
#define SCB_SHPR3 0xE000ED20u
#define NVIC_ISER0 0xE000E100u
#define NVIC_IPR(n) (*(volatile uint8_t *)(0xE000E400u + (n)))
#define TIMER0 0x40000000u
#define TIMER1 0x40001000u
#define TIMER2 0x40002000u /* the dual timer, free-running: polled by main */
#define T_CTRL 0x00u
#define T_VALUE 0x04u
#define T_RELOAD 0x08u
#define T_INTCLEAR 0x0Cu
#define TIMER0_IRQ 8u
#define TIMER1_IRQ 9u
#define DEFAULT_INTERRUPTS 400u

extern uint32_t _sdata, _ebss, _estack, __backstep_image_start, __backstep_image_end;

static uint32_t g_hash = 0x811C9DC5u;
static uint32_t g_rng = 1u;
static volatile uint32_t g_interrupts;
static volatile uint32_t g_flags[3];
static volatile uint32_t g_shared[16];
static uint32_t g_buf[64];
static uint32_t g_crash;
static uint32_t g_ambiguous;
/* Used by name from the wait loop's assembly. */
volatile uint32_t g_spin_count __attribute__((used));
volatile uint32_t g_spin_stop __attribute__((used));
static uint32_t g_target = DEFAULT_INTERRUPTS;
static void (*volatile g_callback)(void);

static struct bs_mmio_event g_log[16384] BS_STATE;
static struct bs_irq_event g_irqs[2048] BS_STATE;
static uint8_t g_ram[16384] BS_STATE;

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

#ifdef BS_TEST_FPU
static volatile float g_fshared[8];

static uint32_t float_bits(float f)
{
    union {
        float f;
        uint32_t u;
    } v;
    v.f = f;
    return v.u;
}

/* Floating-point work: arithmetic, division, square root, conversions,
 * compares; the result goes into the hash bit for bit. */
static float float_work(uint32_t seed, unsigned n)
{
    float acc = (float)(seed & 0xFFFFu) / 3.0f;
    unsigned i;
    for (i = 0; i < n; ++i) {
        float x = (float)(int32_t)(seed ^ (i * 0x9E3779B9u)) * 1.0e-6f;
        acc = acc * 0.999f + __builtin_sqrtf(x * x + 1.0f) / (acc + 2.5f);
        if (acc > 1000.0f) {
            acc -= (float)(uint32_t)(acc / 7.0f);
        }
    }
    return acc;
}
#endif

void systick_isr(void);
void timer0_isr(void);
void timer1_isr(void);
void systick_stub(void);
void timer0_stub(void);
void timer1_stub(void);

void systick_isr(void)
{
    uint32_t r = next_random();
#ifdef BS_TEST_FPU
    g_fshared[r & 7u] += float_work(r, 3u); /* lazy preservation of the main code's FP state */
#endif
    REG32(SYST_RVR) = 3000u + (r % 9000u); /* next tick: random */
    REG32(SYST_CVR) = 0u;
    g_shared[r & 15u] += REG32(TIMER2 + T_VALUE);
    g_flags[0]++;
    g_spin_stop = 1u;
    g_interrupts++;
}

void timer0_isr(void)
{
    uint32_t r = next_random();
    REG32(TIMER0 + T_INTCLEAR) = 1u;
    REG32(TIMER0 + T_RELOAD) = 120u + (r % 360u);
    g_shared[(r >> 4) & 15u] ^= REG32(TIMER0 + T_VALUE) + bs_crc32((const void *)g_shared, 16u);
    g_flags[1]++;
    g_spin_stop = 1u;
    g_interrupts++;
}

void timer1_isr(void)
{
    uint32_t r = next_random(), i, acc = 0;
    REG32(TIMER1 + T_INTCLEAR) = 1u;
    REG32(TIMER1 + T_RELOAD) = 160u + (r % 480u);
    for (i = 0; i < 24u; ++i) { /* long enough to be preempted */
        acc = acc * 31u + g_shared[i & 15u];
    }
#ifdef BS_TEST_FPU
    {
        /* Floating-point work SysTick can preempt: a nested extended frame. */
        float f = float_work(acc, 12u);
        acc ^= float_bits(f + g_fshared[(r >> 3) & 7u]);
    }
#endif
    g_shared[r & 15u] = acc ^ REG32(TIMER1 + T_VALUE);
    g_flags[2]++;
    g_spin_stop = 1u;
    g_interrupts++;
    if (g_crash != 0u && g_interrupts > g_target * 3u / 8u) {
        g_callback = (void (*)(void))0x00001000u; /* not code: the main code faults */
    }
}

BS_IRQ_STUB(systick_stub, systick_isr)
BS_IRQ_STUB(timer0_stub, timer0_isr)
BS_IRQ_STUB(timer1_stub, timer1_isr)

void SysTick_Handler(void) __attribute__((naked));
void SysTick_Handler(void)
{
    __asm__ volatile("b systick_stub");
}

__attribute__((section(".isr_vector.device"), used))
const void *const g_device_vectors[TIMER1_IRQ + 1u] = {
    [TIMER0_IRQ] = (const void *)timer0_stub,
    [TIMER1_IRQ] = (const void *)timer1_stub,
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

/*
 * Spin until an interrupt: every iteration has the same registers, flags
 * and stack at four of its eight instructions; only g_spin_count, in
 * static RAM (not in the key), tells them apart. Afterwards one timer
 * read per iteration: an interrupt replayed in the wrong iteration
 * changes the number of peripheral loads, which replay notices at once.
 */
static void ambiguous_wait(void)
{
    uint32_t n;
    g_spin_count = 0u;
    g_spin_stop = 0u;
    __asm__ volatile("1: ldr r0, =g_spin_count\n\t"
                     "ldr r1, [r0]\n\t"
                     "adds r1, r1, #1\n\t"
                     "str r1, [r0]\n\t"
                     "ldr r0, =g_spin_stop\n\t"
                     "ldr r1, [r0]\n\t"
                     "cmp r1, #0\n\t"
                     "beq 1b\n\t" ::: "r0", "r1", "cc", "memory");
    for (n = g_spin_count; n != 0u; --n) {
        fold(REG32(TIMER2 + T_VALUE));
    }
}

static void round_of_work(uint32_t round)
{
    uint32_t i, j;
    for (i = 0; i < 64u; ++i) {
        g_buf[i] = next_random() ^ g_shared[i & 15u];
    }
    for (i = 1; i < 32u; ++i) { /* insertion sort of half the buffer */
        uint32_t v = g_buf[i];
        for (j = i; j > 0u && g_buf[j - 1u] > v; --j) {
            g_buf[j] = g_buf[j - 1u];
        }
        g_buf[j] = v;
    }
    fold(bs_crc32(g_buf, sizeof g_buf));
#ifdef BS_TEST_FPU
    {
        /* Values live in FP registers across the interrupts that arrive
         * during this loop. */
        float a = (float)round, b = 0.5f;
        for (i = 0; i < 32u; ++i) {
            a = a * 1.0001f + (float)(g_buf[i] & 0xFFFFu) / 7.0f;
            b = __builtin_sqrtf(a + b * b);
            fold(float_bits(b) ^ REG32(TIMER2 + T_VALUE)); /* trapped load in FP context */
        }
        fold(float_bits(a + g_fshared[round & 7u]));
        fold(__builtin_arm_get_fpscr());
    }
#endif
    fold(deep(6u + (round & 3u)));
    for (i = 0; i < 6u; ++i) {
        fold(REG32(TIMER2 + T_VALUE)); /* polling from the main code */
    }
    wait_flag(round % 3u);
    if (g_ambiguous != 0u) {
        ambiguous_wait();
    }
    g_callback(); /* WHY-CRASH-CALL: the call through the corrupted pointer */
}

static uint32_t g_handle;
static uint32_t g_write_ok;

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
    g_write_ok = g_handle != 0xFFFFFFFFu;
    bytes = bs_record_write(put, 0);
    semihost_close(g_handle);
    test_check(bytes != 0u && g_write_ok != 0u, "recording.bsr written");
    test_puts("interrupts ");
    test_put_u32(g_interrupts);
    test_puts(", hash ");
    test_put_hex(g_hash);
    test_puts(", recording ");
    test_put_u32(bytes);
    test_puts(" bytes\n");
}

/* A genuine fault reaches the capture's HardFault path and ends here: the
 * recording ends at the crash (a crash dump). */
void bs_mmio_trap_fatal(uint32_t pc, uint32_t reason)
{
    bs_record_end();
    test_puts("crash at ");
    test_put_hex(pc);
    test_puts(" reason ");
    test_put_u32(reason);
    test_puts("\n");
    write_recording();
    test_finish();
    for (;;) {
    }
}

int main(void)
{
    char cmdline[32] = {0};
    struct bs_record_config cfg;
    uint32_t round = 0, i;

    test_uart_init();
    if (semihost_get_cmdline(cmdline, sizeof cmdline) == 0) {
        /* "s=", "c=" or "a=<seed>", then optionally ":n=<interrupts>" */
        if ((cmdline[0] == 's' || cmdline[0] == 'c' || cmdline[0] == 'a') && cmdline[1] == '=') {
            uint32_t seed = 0;
            for (i = 2; cmdline[i] >= '0' && cmdline[i] <= '9'; ++i) {
                seed = seed * 10u + (uint32_t)(cmdline[i] - '0');
            }
            if (cmdline[0] == 'c') {
                g_crash = 1u;
                g_rng = (seed + 7u) * 0x85EBCA6Bu;
            } else {
                g_ambiguous = cmdline[0] == 'a';
                g_rng = (seed + 1u) * 0x9E3779B1u;
            }
            if (cmdline[i] == ':' && cmdline[i + 1] == 'n' && cmdline[i + 2] == '=') {
                g_target = 0;
                for (i += 3; cmdline[i] >= '0' && cmdline[i] <= '9'; ++i) {
                    g_target = g_target * 10u + (uint32_t)(cmdline[i] - '0');
                }
            }
        }
    }
    g_rng |= 1u;
    g_callback = callback_ok;

    __asm__ volatile("cpsid i" ::: "memory");
    REG32(SCB_SHPR3) = (REG32(SCB_SHPR3) & 0x00FFFFFFu) | (0x40u << 24); /* SysTick */
    NVIC_IPR(TIMER0_IRQ) = 0x80u;
    NVIC_IPR(TIMER1_IRQ) = 0xC0u;
    REG32(TIMER2 + 0x00u) = 0xFFFFFFFFu; /* dual timer: free-running */
    REG32(TIMER2 + 0x08u) = 0x82u;
    REG32(TIMER0 + T_RELOAD) = 200u;
    REG32(TIMER0 + T_CTRL) = 1u | (1u << 3);
    REG32(TIMER1 + T_RELOAD) = 300u;
    REG32(TIMER1 + T_CTRL) = 1u | (1u << 3);
    REG32(NVIC_ISER0) = (1u << TIMER0_IRQ) | (1u << TIMER1_IRQ);
    REG32(SYST_RVR) = 5000u;
    REG32(SYST_CVR) = 0u;
    REG32(SYST_CSR) = 7u; /* processor clock, interrupt, enable */

    cfg.target_id = BS_TARGET_QEMU_MPS2_AN386;
    cfg.image_start = &__backstep_image_start;
    cfg.image_end = &__backstep_image_end;
    cfg.ram_start = &_sdata;
    cfg.ram_end = &_ebss;
    cfg.stack_top = &_estack;
    cfg.stack_bottom = (const void *)((uintptr_t)&_estack - 8192u); /* the stack area */
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
    while (g_interrupts < g_target) {
        round_of_work(round++);
    }
    bs_record_end();
    /* ---- not recorded ---- */

    REG32(SYST_CSR) = 0u;
    write_recording();
    test_finish();
    return 0;
}
