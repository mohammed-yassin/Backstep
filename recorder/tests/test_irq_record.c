/*
 * Host unit tests for interrupt-context recording (src/irq_record.c).
 *
 * The on-target behaviour is checked against QEMU's instruction trace by
 * scripts/irq-position-check.py; these tests cover the logic directly,
 * including the rare case QEMU almost never produces: a higher-priority
 * interrupt arriving before the outer stub's first instruction.
 *
 * SPDX-License-Identifier: MIT
 */
#include <stdio.h>
#include <string.h>

#include "backstep_irq.h"

static int g_checks;
static int g_failures;

#define EXPECT(cond, msg)                                                    \
    do {                                                                     \
        g_checks++;                                                          \
        if (!(cond)) {                                                       \
            g_failures++;                                                    \
            printf("FAIL %s:%d: %s\n", __FILE__, __LINE__, msg);             \
        }                                                                    \
    } while (0)

/* A fake stack: exception frame (8 words) followed by live stack words. */
static uint32_t g_stack[8 + 1 + 64];
static uint32_t g_callee[8];
static struct bs_irq_event g_log[4];
static volatile uint32_t g_reads;

static void reset(void)
{
    unsigned i;
    for (i = 0; i < sizeof g_stack / sizeof g_stack[0]; ++i) {
        g_stack[i] = 0x1000u + i;
    }
    for (i = 0; i < 8u; ++i) {
        g_callee[i] = 0x400u + i; /* r4..r11 */
    }
    g_stack[0] = 0xA0; /* r0 */
    g_stack[1] = 0xA1;
    g_stack[2] = 0xA2;
    g_stack[3] = 0xA3;
    g_stack[4] = 0xAC;        /* r12 */
    g_stack[5] = 0x0800ABCDu; /* lr */
    g_stack[6] = 0x08001234u; /* pc */
    g_stack[7] = 0x61000000u; /* xpsr, thread mode */
    memset(g_log, 0, sizeof g_log);
    g_reads = 0;
}

static void test_context_fields(void)
{
    const struct bs_irq_event *e = &g_log[0];
    unsigned i;
    int ok = 1;

    reset();
    g_reads = 7;
    bs_irq_record_start(g_log, 4, &g_reads, &g_stack[8 + 64]);
    bs_irq_record(g_callee, g_stack, 15u, 0x2001u);
    EXPECT(bs_irq_record_count() == 1u, "one event counted");
    EXPECT(e->exception == 15u, "exception number from IPSR");
    EXPECT(e->r[0] == 0xA0 && e->r[3] == 0xA3 && e->r[12] == 0xAC,
           "r0-r3, r12 from the hardware frame");
    for (i = 0; i < 8u; ++i) {
        ok &= e->r[4 + i] == 0x400u + i;
    }
    EXPECT(ok, "r4-r11 from the stub's saved registers");
    EXPECT(e->lr == 0x0800ABCDu && e->pc == 0x08001234u, "lr and pc");
    EXPECT(e->xpsr == 0x61000000u, "xpsr");
    EXPECT(e->mmio_index == 7u, "read count from the counter");
    EXPECT((uintptr_t)&g_stack[8] - (uintptr_t)g_stack == 32u, "test layout");
    EXPECT(e->stack_hash == bs_stack_hash(&g_stack[8], BS_STACK_HASH_BYTES / 4u),
           "stack hash over 128 bytes above the frame");
}

static void test_alignment_padding(void)
{
    const struct bs_irq_event *e = &g_log[0];
    reset();
    g_stack[7] = 0x61000200u; /* bit 9: the hardware added 4 bytes of padding */
    bs_irq_record_start(g_log, 4, &g_reads, &g_stack[8 + 64]);
    bs_irq_record(g_callee, g_stack, 15u, 0x2001u);
    EXPECT(e->xpsr == 0x61000000u, "padding bit cleared from xpsr");
    EXPECT(e->stack_hash == bs_stack_hash(&g_stack[9], BS_STACK_HASH_BYTES / 4u),
           "stack hash starts after the padding word");
}

static void test_stack_top_clamps_hash(void)
{
    reset();
    bs_irq_record_start(g_log, 4, &g_reads, &g_stack[8 + 5]);
    bs_irq_record(g_callee, g_stack, 15u, 0x2001u);
    EXPECT(g_log[0].stack_hash == bs_stack_hash(&g_stack[8], 5u),
           "hash stops at the top of the stack");
}

static void test_preempted_at_stub_entry(void)
{
    /* Outer exception 24 is entered with 3 reads made; before its stub runs,
     * SysTick (15) preempts at the stub's first instruction and its handler
     * makes 2 more reads. The outer event must still say 3. */
    reset();
    bs_irq_record_start(g_log, 4, &g_reads, &g_stack[8 + 64]);
    g_reads = 3;
    g_stack[6] = 0x00000080u;      /* inner interrupted at the outer stub start */
    g_stack[7] = 0x01000018u;      /* ... while in handler mode, exception 24 */
    bs_irq_record(g_callee, g_stack, 15u, 0x0479u);
    g_reads = 5;                   /* reads made by the inner handler */
    g_stack[6] = 0x08001234u;
    g_stack[7] = 0x61000000u;
    bs_irq_record(g_callee, g_stack, 24u, 0x0081u); /* stub at 0x80, Thumb bit */
    EXPECT(g_log[1].mmio_index == 3u,
           "outer event keeps the read count of its true interruption point");

    /* Same exception number but the inner did NOT interrupt our stub. */
    g_stack[6] = 0x00000100u;
    g_stack[7] = 0x01000018u;
    bs_irq_record(g_callee, g_stack, 15u, 0x0479u);
    g_reads = 9;
    g_stack[6] = 0x08001234u;
    bs_irq_record(g_callee, g_stack, 24u, 0x0081u);
    EXPECT(g_log[3].mmio_index == 9u, "otherwise the current count is used");
}

static void test_capacity(void)
{
    struct bs_irq_event small[1];
    reset();
    memset(small, 0, sizeof small);
    bs_irq_record_start(small, 1, &g_reads, &g_stack[8 + 64]);
    bs_irq_record(g_callee, g_stack, 15u, 0x2001u);
    bs_irq_record(g_callee, g_stack, 15u, 0x2001u);
    bs_irq_record(g_callee, g_stack, 15u, 0x2001u);
    EXPECT(bs_irq_record_count() == 3u, "overflowing events are still counted");
    EXPECT(small[0].pc == 0x08001234u, "first event kept");
}

static void test_without_log_and_after_stop(void)
{
    reset();
    /* Without a log, events are only counted (the recording then reports
     * them as dropped instead of missing them). */
    bs_irq_record_start(0, 4, &g_reads, &g_stack[8 + 64]);
    bs_irq_record(g_callee, g_stack, 15u, 0x2001u);
    bs_irq_record(g_callee, g_stack, 15u, 0x2001u);
    EXPECT(bs_irq_record_count() == 2u, "counted without a log");
    /* After stop nothing is counted or written. */
    bs_irq_record_start(g_log, 4, &g_reads, &g_stack[8 + 64]);
    bs_irq_record(g_callee, g_stack, 15u, 0x2001u);
    bs_irq_record_stop();
    g_stack[6] = 0x08005555u;
    bs_irq_record(g_callee, g_stack, 15u, 0x2001u);
    EXPECT(bs_irq_record_count() == 1u, "not counted after stop");
    EXPECT(g_log[1].pc == 0u, "not written after stop");
    EXPECT(g_log[0].pc == 0x08001234u, "the event before stop is kept");
}

static void test_hash_known_value(void)
{
    static const uint32_t words[2] = {0x00000000u, 0xFFFFFFFFu};
    /* FNV-1a, 32-bit, applied per word. */
    uint32_t h = 0x811C9DC5u;
    h = (h ^ words[0]) * 0x01000193u;
    h = (h ^ words[1]) * 0x01000193u;
    EXPECT(bs_stack_hash(words, 2u) == h, "FNV-1a word hash");
    EXPECT(bs_stack_hash(words, 0u) == 0x811C9DC5u, "empty hash is the offset basis");
}

int main(void)
{
    test_context_fields();
    test_alignment_padding();
    test_stack_top_clamps_hash();
    test_preempted_at_stub_entry();
    test_capacity();
    test_without_log_and_after_stop();
    test_hash_known_value();
    printf("irq_record tests: %d checks, %d failures\n", g_checks, g_failures);
    return g_failures == 0 ? 0 : 1;
}
