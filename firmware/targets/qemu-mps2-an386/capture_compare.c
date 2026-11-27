/*
 * M1: run the same workload under one capture mechanism and print a
 * digest of the captured reads. Built twice:
 *   capture_compare_mpu.elf  - GCC, MPU trap
 *   capture_compare_cc.elf   - workload built by Clang + Backstep pass
 * scripts/capture-compare.sh checks that both digests are identical and
 * that every read the workload made was captured.
 *
 * SPDX-License-Identifier: MIT
 */
#include <stdint.h>

#include "backstep.h"
#include "backstep_mmio.h"
#include "capture_workload.h"
#include "qemu_test.h"
#include "vectors.h"

#if defined(USE_CC)
#define MECHANISM "cc"
#else
#define MECHANISM "mpu"
__attribute__((naked)) void MemManage_Handler(void)
{
    __asm__ volatile("b bs_memmanage_handler");
}
__attribute__((naked)) void HardFault_Handler(void)
{
    __asm__ volatile("b bs_hardfault_handler");
}
#endif

#define RAM_WINDOW 0x20200000u
#define LOG_CAPACITY 2048u
#define SYST_CSR QT_REG32(0xE000E010u)
#define SYST_RVR QT_REG32(0xE000E014u)
#define SYST_CVR QT_REG32(0xE000E018u)

static struct bs_mmio_event g_log[LOG_CAPACITY];

static void capture_on(uint32_t base, uint32_t log2)
{
#if defined(USE_CC)
    bs_cc_capture_enable(g_log, LOG_CAPACITY, base, 1u << log2);
#else
    (void)bs_mmio_trap_enable_region(g_log, LOG_CAPACITY, base, log2);
#endif
}

static uint32_t capture_off(void)
{
#if defined(USE_CC)
    bs_cc_capture_disable();
    return bs_cc_capture_stats()->loads;
#else
    uint32_t n = bs_mmio_trap_stats()->loads;
    bs_mmio_trap_disable();
    return n;
#endif
}

static uint32_t digest(uint32_t n)
{
    uint32_t i, crc = 0u;
    for (i = 0; i < n && i < LOG_CAPACITY; ++i) {
        crc = bs_crc32_update(crc, &g_log[i].address, 4u);
        crc = bs_crc32_update(crc, &g_log[i].value, 4u);
        crc = bs_crc32_update(crc, &g_log[i].size, 1u);
    }
    return crc;
}

static void report(const char *phase, uint32_t reads, uint32_t captured,
                   uint32_t checksum)
{
    test_puts("DIGEST ");
    test_puts(phase);
    test_puts(" reads=");
    test_put_u32(reads);
    test_puts(" captured=");
    test_put_u32(captured);
    test_puts(" log_crc=");
    test_put_hex(digest(captured));
    test_puts(" result=");
    test_put_hex(checksum);
    test_puts("\n");
    test_check_eq(captured, reads, "every read captured");
}

static uint32_t ticks_for_reads(uint32_t n, int capture)
{
    uint32_t t0, t1;
    SYST_RVR = 0x00FFFFFFu;
    SYST_CVR = 0u;
    SYST_CSR = 0x5u;
    while (SYST_CVR == 0u) {
    }
    if (capture) {
        capture_on(0x40000000u, 29u);
    }
    t0 = SYST_CVR;
    (void)workload_repeat_reads(n);
    t1 = SYST_CVR;
    if (capture) {
        (void)capture_off();
    }
    SYST_CSR = 0u;
    return (t0 - t1) & 0x00FFFFFFu;
}

int main(void)
{
    uint32_t reads, captured, checksum = 0u, with, without;

    test_uart_init();
    test_puts("Backstep M1 capture comparison (" MECHANISM ")\n");

    capture_on(0x40000000u, 29u);
    reads = workload_peripheral(&checksum);
    captured = capture_off();
    report("peripheral", reads, captured, checksum);

    capture_on(RAM_WINDOW, 20u);
    reads = workload_ram(RAM_WINDOW, &checksum);
    captured = capture_off();
    report("ram", reads, captured, checksum);

    with = ticks_for_reads(1000u, 1);
    without = ticks_for_reads(1000u, 0);
    /* -icount shift=0 and a 25 MHz SysTick: 1 tick = 40 instructions. */
    test_puts("MEASURE " MECHANISM " instructions_per_1000_reads capture_on=");
    test_put_u32(with * 40u);
    test_puts(" capture_off=");
    test_put_u32(without * 40u);
    test_puts("\n");
    test_finish();
    return 0;
}
