/*
 * Runtime for compile-time peripheral-read capture (EXPERIMENTAL).
 * The Clang pass in tools/clang-mmio-pass redirects volatile loads here.
 * This file itself must be compiled WITHOUT the pass.
 *
 * SPDX-License-Identifier: MIT
 */
#include "backstep_mmio.h"

static struct bs_mmio_event *s_log;
static uint32_t s_capacity;
static uint32_t s_base;
static uint32_t s_size; /* 0 = capture disabled */
static volatile struct bs_mmio_trap_stats s_stats;

void bs_cc_capture_enable(struct bs_mmio_event *log, uint32_t capacity,
                          uint32_t base, uint32_t size)
{
    s_log = log;
    s_capacity = capacity;
    s_base = base;
    s_stats.loads = 0u;
    s_stats.stores = 0u;
    s_stats.dropped = 0u;
    s_stats.unsupported = 0u;
    s_size = size; /* last: enables capture */
}

void bs_cc_capture_disable(void)
{
    s_size = 0u;
}

const volatile struct bs_mmio_trap_stats *bs_cc_capture_stats(void)
{
    return &s_stats;
}

static void record(uint32_t addr, uint32_t value, uint8_t size, void *ret)
{
    uint32_t n;
    if ((addr - s_base) >= s_size) {
        return;
    }
    n = s_stats.loads;
    if (n < s_capacity) {
        s_log[n].address = addr;
        s_log[n].value = value;
        s_log[n].pc = (uint32_t)(uintptr_t)ret;
        s_log[n].size = size;
    } else {
        s_stats.dropped++;
    }
    s_stats.loads = n + 1u;
}

uint8_t __bs_mmio_load8(const volatile uint8_t *p)
{
    uint8_t v = *p;
    record((uint32_t)(uintptr_t)p, v, 1u, __builtin_return_address(0));
    return v;
}

uint16_t __bs_mmio_load16(const volatile uint16_t *p)
{
    uint16_t v = *p;
    record((uint32_t)(uintptr_t)p, v, 2u, __builtin_return_address(0));
    return v;
}

uint32_t __bs_mmio_load32(const volatile uint32_t *p)
{
    uint32_t v = *p;
    record((uint32_t)(uintptr_t)p, v, 4u, __builtin_return_address(0));
    return v;
}
