/*
 * M1 test: does a starting-state snapshot contain everything execution
 * depends on?
 *
 * The firmware builds up state in every place a program keeps it: globals
 * (.data and .bss), a local array on the stack, locals held in callee-saved
 * registers across the call, and System Control Space settings (exception
 * priorities, enabled interrupt, vector table base). Then it takes a
 * snapshot and runs a workload that depends on all of it.
 *
 *   Run 1 (normal): writes snapshot.bin, then the workload result to
 *                   result_original.bin.
 *   Run 2 (fresh QEMU): scripts/snapshot-restore-test.sh loads only
 *                   snapshot.bin into a fresh machine through GDB and
 *                   resumes it; the workload writes result_restored.bin.
 * The two results must be identical.
 *
 * SPDX-License-Identifier: MIT
 */
#include <stdint.h>

#include "backstep.h"
#include "backstep_snapshot.h"
#include "qemu_test.h"
#include "vectors.h"

extern uint32_t _sdata, _ebss, _estack;

#define SCB_VTOR QT_REG32(0xE000ED08u)
#define SCB_SHPR3 QT_REG32(0xE000ED20u)
#define NVIC_ISER0 QT_REG32(0xE000E100u)
#define NVIC_IPR8 (*(volatile uint8_t *)0xE000E408u)

static uint32_t g_data[64] = {1u, 2u, 3u}; /* .data */
static uint32_t g_bss[256];                 /* .bss */
static uint32_t g_rng = 0xC0FFEE01u;        /* .data */
/* Backstep's working memory lives outside the program's saved RAM. */
static struct bs_cpu_state g_cpu __attribute__((section(".backstep")));
static struct bs_snapshot_header g_hdr __attribute__((section(".backstep")));
static int g_saved_ok __attribute__((section(".backstep")));

/* The debugger enters the restored machine through these; reference them
 * so the linker keeps them. */
__attribute__((used, section(".backstep.keep"))) static const void *const g_resume_entry[2] = {
    (const void *)bs_snapshot_resume, (const void *)&bs_resume_header};

static uint32_t xorshift(void)
{
    g_rng ^= g_rng << 13;
    g_rng ^= g_rng >> 17;
    g_rng ^= g_rng << 5;
    return g_rng;
}

/* Depends on globals, the caller's stack array, register-held values,
 * and SCS state. */
static __attribute__((noinline)) uint32_t workload(const uint32_t *stack_arr,
                                                   uint32_t reg_a, uint32_t reg_b)
{
    uint32_t i, acc = reg_a ^ (reg_b << 7);
    for (i = 0; i < 2000u; ++i) {
        uint32_t r = xorshift();
        g_bss[r & 255u] += g_data[r % 64u] + stack_arr[(r >> 8) & 31u];
        acc = (acc * 31u) ^ g_bss[(r >> 16) & 255u];
    }
    acc ^= SCB_SHPR3 ^ (NVIC_ISER0 << 3) ^ ((uint32_t)NVIC_IPR8 << 11) ^ SCB_VTOR;
    return acc ^ bs_crc32(g_bss, sizeof g_bss);
}

/* Runs inside bs_snapshot_take(), below the caller's stack. Touches only
 * registers, its own (dead-stack) frame and the .backstep section. */
static void save_snapshot(const struct bs_cpu_state *cpu)
{
    static const char k_snap[] = "snapshot.bin";
    struct bs_region regions[2];
    uint32_t h, i;
    int ok = 1;

    regions[0].address = (uint32_t)(uintptr_t)&_sdata;
    regions[0].length = (uint32_t)((uintptr_t)&_ebss - (uintptr_t)&_sdata);
    regions[1].address = cpu->sp;
    regions[1].length = (uint32_t)((uintptr_t)&_estack - cpu->sp);
    bs_snapshot_header_build(&g_hdr, cpu, regions, 2u);
    h = semihost_open_write(k_snap, sizeof k_snap - 1u);
    ok &= h != 0xFFFFFFFFu;
    ok &= semihost_write(h, &g_hdr, sizeof g_hdr) == 0;
    for (i = 0; i < 2u; ++i) {
        ok &= semihost_write(h, (const void *)(uintptr_t)regions[i].address,
                             regions[i].length) == 0;
    }
    semihost_close(h);
    g_saved_ok = ok;
}

static void write_result(const char *name, uint32_t name_len, uint32_t value)
{
    test_check(semihost_write_file(name, name_len, &value, sizeof value) == 0,
               "result written");
}

int main(void)
{
    static const char k_orig[] = "result_original.bin";
    static const char k_rest[] = "result_restored.bin";
    uint32_t stack_arr[32];
    uint32_t reg_a, reg_b, i;

    test_uart_init();

    /* Build state everywhere. */
    for (i = 3; i < 64u; ++i) {
        g_data[i] = xorshift();
    }
    for (i = 0; i < 256u; ++i) {
        g_bss[i] = xorshift() & 0xFFFFu;
    }
    for (i = 0; i < 32u; ++i) {
        stack_arr[i] = xorshift();
    }
    reg_a = xorshift();
    reg_b = xorshift();
    SCB_SHPR3 = 0x40800000u;      /* SysTick 0x40, PendSV 0x80 */
    NVIC_IPR8 = 0xA0u;            /* IRQ8 priority */
    NVIC_ISER0 = 1u << 8;         /* IRQ8 enabled (never fires here) */
    __asm__ volatile("cpsid i" ::: "memory");

    if (bs_snapshot_take(&g_cpu, save_snapshot) == 0) {
        test_check(g_saved_ok, "snapshot.bin written");
        test_puts("snapshot taken, running workload (original)\n");
        write_result(k_orig, sizeof k_orig - 1u, workload(stack_arr, reg_a, reg_b));
    } else {
        /* Restored machine: no UART setup happened here, so the result
         * goes out through semihosting only (printing still works since
         * QEMU's UART needs no setup to accept data). */
        write_result(k_rest, sizeof k_rest - 1u, workload(stack_arr, reg_a, reg_b));
    }
    test_finish();
    return 0;
}
