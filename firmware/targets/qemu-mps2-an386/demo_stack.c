/*
 * Example bug for `backstep why` (M6): a task's stack overflows into the
 * task's own bookkeeping, which sits just below the stack array.
 *
 * The task runs on its own small stack (the process stack, PSP, as an
 * RTOS would set it up) and walks a tree recursively. One input makes the
 * walk deeper than the stack allows; the pushes of the deepest calls land
 * in g_task.state. The main code later finds the state corrupted and ends
 * the recording there.
 *
 *   $ backstep why demo_stack.elf recording.bsr g_task.state
 *   -> the push in walk(), the recursive function (WHY-STACK-PUSH)
 *
 * SPDX-License-Identifier: MIT
 */
#include <stdint.h>

#include "backstep.h"
#include "backstep_record.h"
#include "qemu_test.h"
#include "vectors.h"

#define REG32(a) (*(volatile uint32_t *)(a))
#define TIMER2 0x40002000u /* the dual timer, free-running: the "input" */
#define STATE_OK 0x5AFEu
#define STACK_WORDS 48u

extern uint32_t _sdata, _ebss, _estack, __backstep_image_start, __backstep_image_end;

/* A task control block: its name, its state, then its stack (growing
 * down towards the state). */
static struct task {
    char name[128];
    volatile uint32_t state;
    uint32_t stack[STACK_WORDS];
} g_task = {"walker", STATE_OK, {0}};

static uint32_t g_result;

static struct bs_mmio_event g_log[256] BS_STATE;
static uint8_t g_ram[8192] BS_STATE;

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

/* Sum of a "tree" of the given depth: each level keeps a few locals. */
static uint32_t __attribute__((noinline)) walk(uint32_t depth, uint32_t seed)
{ /* WHY-STACK-PUSH: the prologue's push, the deepest calls' land in g_task.state */
    volatile uint32_t local[4];
    uint32_t i, sum = seed;
    for (i = 0; i < 4u; ++i) {
        local[i] = seed * (i + 3u) + depth;
    }
    if (depth > 0u) {
        sum += walk(depth - 1u, seed ^ local[depth & 3u]);
    }
    return sum + local[0] + local[3];
}

/* Run fn(depth) on the task's stack: PSP, as an RTOS task would. */
static uint32_t run_task(uint32_t depth)
{
    uint32_t result;
    __asm__ volatile("msr psp, %0\n\t"
                     "mov r0, #2\n\t"
                     "msr control, r0\n\t" /* thread mode on PSP */
                     "isb\n\t" ::"r"(&g_task.stack[STACK_WORDS])
                     : "r0", "memory");
    result = walk(depth, 0x1234u);
    __asm__ volatile("mov r0, #0\n\t"
                     "msr control, r0\n\t" /* back to MSP */
                     "isb\n\t" ::
                         : "r0", "memory");
    return result;
}

static uint32_t g_handle;
static uint32_t g_write_ok = 1u;

static void put(const void *data, uint32_t len, void *ctx)
{
    (void)ctx;
    g_write_ok &= semihost_write(g_handle, data, len) == 0;
}

static void finish(int bug_shown, const char *what)
{
    static const char k_name[] = "recording.bsr";
    uint32_t bytes;
    bs_record_end();
    g_handle = semihost_open_write(k_name, sizeof k_name - 1u);
    bytes = bs_record_write(put, 0);
    semihost_close(g_handle);
    test_check(bytes != 0u && g_write_ok != 0u, "recording.bsr written");
    test_check(bug_shown, "the bug showed");
    test_puts(what);
    test_finish();
}

int main(void)
{
    struct bs_record_config cfg;
    uint32_t round;

    test_uart_init();
    REG32(TIMER2 + 0x00u) = 0xFFFFFFFFu;
    REG32(TIMER2 + 0x08u) = 0x82u;

    cfg.target_id = BS_TARGET_QEMU_MPS2_AN386;
    cfg.image_start = &__backstep_image_start;
    cfg.image_end = &__backstep_image_end;
    cfg.ram_start = &_sdata;
    cfg.ram_end = &_ebss;
    cfg.stack_top = &_estack;
    cfg.stack_bottom = (const void *)((uintptr_t)&_estack - 4096u);
    cfg.log = g_log;
    cfg.log_capacity = sizeof g_log / sizeof g_log[0];
    cfg.ram_buffer = g_ram;
    cfg.ram_buffer_capacity = sizeof g_ram;
    cfg.irq_log = 0;
    cfg.irq_capacity = 0u;
    __asm__ volatile("cpsid i" ::: "memory"); /* no interrupts in this example */

    if (bs_record_begin(&cfg) != 0) {
        test_check(0, "MPU present");
        test_finish();
    }
    /* ---- recorded ---- */
    for (round = 0; round < 40u; ++round) {
        /* The depth comes from the "input"; one value is too deep. */
        uint32_t depth = 2u + (REG32(TIMER2 + 0x04u) >> 3) % 4u + (round == 23u ? 6u : 0u);
        g_result += run_task(depth);
        if (g_task.state != STATE_OK) {
            finish(1, "demo: the task's state was corrupted; recording written\n");
        }
    }
    finish(0, "demo: the bug did not show\n");
    return 0;
}
