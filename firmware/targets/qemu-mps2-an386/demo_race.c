/*
 * Example bug for `backstep why` (M6): a race between the main code and
 * an interrupt handler that both count events in one variable.
 *
 * The main code adds to g_events with a read, some work, and a write; the
 * timer interrupt adds to it too. When the interrupt lands between the
 * main code's read and write, its addition is overwritten: a lost update.
 * The main code checks the total against its own counts and ends the
 * recording when they disagree.
 *
 *   $ backstep why demo_race.elf recording.bsr g_events
 *   -> lost update: read at WHY-RACE-READ, the handler's write at
 *      WHY-RACE-ISR, overwritten by WHY-RACE-STORE
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
#define NVIC_ISER0 0xE000E100u
#define NVIC_IPR(n) (*(volatile uint8_t *)(0xE000E400u + (n)))
#define TIMER0 0x40000000u
#define T_CTRL 0x00u
#define T_RELOAD 0x08u
#define T_INTCLEAR 0x0Cu
#define TIMER0_IRQ 8u

extern uint32_t _sdata, _ebss, _estack, __backstep_image_start, __backstep_image_end;

static volatile uint32_t g_events;      /* shared: the bug */
static volatile uint32_t g_isr_events;  /* counted by the handler alone */
static uint32_t g_main_events, g_work;

static struct bs_mmio_event g_log[512] BS_STATE;
static struct bs_irq_event g_irqs[512] BS_STATE;
static uint8_t g_ram[12288] BS_STATE;

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

void tick_isr(void);
void tick_stub(void);

void tick_isr(void)
{
    REG32(TIMER0 + T_INTCLEAR) = 1u;
    g_events = g_events + 1u; /* WHY-RACE-ISR */
    g_isr_events = g_isr_events + 1u;
}

BS_IRQ_STUB(tick_stub, tick_isr)

__attribute__((section(".isr_vector.device"), used))
const void *const g_device_vectors[TIMER0_IRQ + 1u] = {
    [TIMER0_IRQ] = (const void *)tick_stub,
};

static void __attribute__((noinline)) add_main_event(void)
{
    uint32_t i, v = g_events; /* WHY-RACE-READ */
    for (i = 0; i < 20u; ++i) { /* some work between the read and the write */
        g_work = g_work * 33u + i;
    }
    g_events = v + 1u; /* WHY-RACE-STORE */
    g_main_events++;
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
    REG32(TIMER0 + T_CTRL) = 0u;
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
    uint32_t round, i;

    test_uart_init();
    __asm__ volatile("cpsid i" ::: "memory");
    NVIC_IPR(TIMER0_IRQ) = 0x80u;
    REG32(TIMER0 + T_RELOAD) = 37u;
    REG32(TIMER0 + T_CTRL) = 1u | (1u << 3);
    REG32(NVIC_ISER0) = 1u << TIMER0_IRQ;

    cfg.target_id = BS_TARGET_QEMU_MPS2_AN386;
    cfg.image_start = &__backstep_image_start;
    cfg.image_end = &__backstep_image_end;
    cfg.ram_start = &_sdata;
    cfg.ram_end = &_ebss;
    cfg.stack_top = &_estack;
    cfg.stack_bottom = (const void *)((uintptr_t)&_estack - 8192u);
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
    for (round = 0; round < 2000u; ++round) {
        add_main_event();
        for (i = 0; i < 30u; ++i) { /* other work, where the interrupt is harmless */
            g_work ^= i;
        }
        if (g_events != g_main_events + g_isr_events) {
            finish(1, "demo: events were lost; recording written\n");
        }
    }
    finish(0, "demo: the bug did not show\n");
    return 0;
}
