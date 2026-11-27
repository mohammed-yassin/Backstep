/*
 * Interrupt-position recording (EXPERIMENTAL). See backstep_irq.h.
 *
 * SPDX-License-Identifier: MIT
 */
#include "backstep_irq.h"

static struct bs_irq_event *s_log;
static uint32_t s_capacity;
static volatile uint32_t s_count;
static const volatile uint32_t *s_mmio;
static const uint32_t *s_stack_top;

uint32_t bs_stack_hash(const uint32_t *words, uint32_t count)
{
    uint32_t h = 0x811C9DC5u;
    while (count-- != 0u) {
        h = (h ^ *words++) * 0x01000193u;
    }
    return h;
}

void bs_irq_record_start(struct bs_irq_event *log, uint32_t capacity,
                         const volatile uint32_t *mmio_counter,
                         const void *stack_top)
{
    s_stack_top = (const uint32_t *)stack_top;
    s_log = log;
    s_capacity = capacity;
    s_mmio = mmio_counter;
    s_count = 0u;
}

uint32_t bs_irq_record_count(void)
{
    return s_count;
}

void bs_irq_record(const uint32_t *callee, const uint32_t *frame, uint32_t ipsr,
                   uint32_t stub_address)
{
    struct bs_irq_event *e;
    uint32_t n = s_count, i, xpsr, mmio;

    if (s_log == 0) {
        return;
    }
    /* Interrupts are masked (the stub's first instruction did it). */
    s_count = n + 1u;
    if (n >= s_capacity) {
        return;
    }

    /* If the previous event interrupted this very stub at its first
     * instruction, it preempted this exception before anything of ours
     * ran: our interruption point had the same peripheral-read count, and
     * any reads since then were made by that nested handler. */
    mmio = s_mmio ? *s_mmio : 0u;
    if (n > 0u && s_log[n - 1u].pc == (stub_address & ~1u) &&
        (s_log[n - 1u].xpsr & 0x1FFu) == (ipsr & 0x1FFu)) {
        mmio = s_log[n - 1u].mmio_index;
    }

    e = &s_log[n];
    xpsr = frame[7];
    e->exception = ipsr & 0x1FFu;
    e->r[0] = frame[0];
    e->r[1] = frame[1];
    e->r[2] = frame[2];
    e->r[3] = frame[3];
    for (i = 0; i < 8u; ++i) {
        e->r[4u + i] = callee[i];
    }
    e->r[12] = frame[4];
    e->lr = frame[5];
    e->pc = frame[6];
    /* Bit 9 of the stacked xPSR says the hardware added 4 bytes of padding
     * to align the frame; it is not part of the interrupted xPSR. */
    e->sp = (uint32_t)(uintptr_t)frame + 32u + ((xpsr & (1u << 9)) ? 4u : 0u);
    e->xpsr = xpsr & ~(1u << 9);
    e->mmio_index = mmio;
    {
        /* Live stack of the interrupted code starts right above the frame. */
        const uint32_t *live = frame + 8 + ((xpsr & (1u << 9)) ? 1 : 0);
        uintptr_t avail = (uintptr_t)s_stack_top > (uintptr_t)live
                              ? ((uintptr_t)s_stack_top - (uintptr_t)live) / 4u
                              : 0u;
        uint32_t words = BS_STACK_HASH_BYTES / 4u;
        if (avail < words) {
            words = (uint32_t)avail;
        }
        e->stack_hash = bs_stack_hash(live, words);
    }
}
