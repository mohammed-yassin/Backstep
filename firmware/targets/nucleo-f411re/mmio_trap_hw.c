/*
 * M1 hardware test: MPU peripheral-read trap on a real STM32F411.
 *
 * QEMU proves the trap logic; this proves it on silicon and measures its
 * cost in CPU cycles:
 *   1. Every load form returns the same value trapped and untrapped, and
 *      is logged once with the right address, size and value. Target:
 *      GPIOB_AFRH (fully writable, byte-addressable; PB8-PB15 are inputs
 *      after reset, so their alternate-function bits have no effect).
 *   2. Stores reach the peripheral.
 *   3. Loads from SysTick at priority 0x80 (MemManage path) and 0
 *      (escalated HardFault path) are all captured.
 *   4. Printing through USART2 works while every UART access is trapped.
 *   5. DWT cycle counter: cycles per trapped vs plain load.
 * Run scripts/hil/m1_nucleo_f411re.py to collect the result.
 *
 * SPDX-License-Identifier: MIT
 */
#include <stdint.h>

#include "backstep_mmio.h"
#include "nucleo_test.h"
#include "vectors.h"

#define GPIOB_BASE 0x40020400u
#define GPIOB_AFRH_ADDR (GPIOB_BASE + 0x24u)
#define FORM_BASE (GPIOB_AFRH_ADDR - 8u) /* forms access base + 8..11 */
#define PATTERN 0x8A6B4C2Du

#define DEMCR NT_REG32(0xE000EDFCu)
#define DWT_CTRL NT_REG32(0xE0001000u)
#define DWT_CYCCNT NT_REG32(0xE0001004u)
#define SYST_CSR NT_REG32(0xE000E010u)
#define SYST_RVR NT_REG32(0xE000E014u)
#define SYST_CVR NT_REG32(0xE000E018u)
#define SCB_SHPR3 NT_REG32(0xE000ED20u)

uint32_t ld16_word_imm(uint32_t base);
uint32_t ld16_byte_imm(uint32_t base);
uint32_t ld16_half_imm(uint32_t base);
uint32_t ld16_word_reg(uint32_t base);
uint32_t ld16_sbyte_reg(uint32_t base);
uint32_t ld16_shalf_reg(uint32_t base);
uint32_t ld32_high_regs(uint32_t base);
uint32_t ld32_negative(uint32_t base);
uint32_t ld32_shifted_reg(uint32_t base);
uint32_t ld32_sbyte_imm(uint32_t base);
void st16_word_imm(uint32_t base, uint32_t value);
void st16_byte_imm(uint32_t base, uint32_t value);

__attribute__((naked)) void MemManage_Handler(void)
{
    __asm__ volatile("b bs_memmanage_handler");
}

__attribute__((naked)) void HardFault_Handler(void)
{
    __asm__ volatile("b bs_hardfault_handler");
}

#define LOG_CAPACITY 4096u
static struct bs_mmio_event g_log[LOG_CAPACITY];
static volatile uint32_t g_isr_reads;
static volatile uint32_t g_isr_bad;

void SysTick_Handler(void)
{
    if (*(volatile uint32_t *)GPIOB_AFRH_ADDR != PATTERN) {
        g_isr_bad++;
    }
    g_isr_reads++;
}

void bs_mmio_trap_fatal(uint32_t pc, uint32_t reason)
{
    bs_mmio_trap_disable();
    test_check(0, "trap reported a fatal condition");
    test_puts("     pc ");
    test_put_hex(pc);
    test_puts(" reason ");
    test_put_u32(reason);
    test_puts("\n");
    test_finish();
}

typedef uint32_t (*form_fn)(uint32_t base);
struct form {
    const char *name;
    form_fn fn;
    uint32_t offset;
    uint8_t size;
};

static const struct form k_forms[] = {
    {"ldr.n imm5", ld16_word_imm, 8, 4},
    {"ldrb.n imm5", ld16_byte_imm, 9, 1},
    {"ldrh.n imm5", ld16_half_imm, 10, 2},
    {"ldr.n register offset", ld16_word_reg, 8, 4},
    {"ldrsb.n register offset", ld16_sbyte_reg, 11, 1},
    {"ldrsh.n register offset", ld16_shalf_reg, 10, 2},
    {"ldr.w r9,[r8]", ld32_high_regs, 8, 4},
    {"ldr negative offset", ld32_negative, 8, 4},
    {"ldr.w [Rn,Rm,lsl #2]", ld32_shifted_reg, 8, 4},
    {"ldrsb.w imm12", ld32_sbyte_imm, 11, 1},
};

static void test_forms(void)
{
    unsigned i;
    for (i = 0; i < sizeof k_forms / sizeof k_forms[0]; ++i) {
        const struct form *f = &k_forms[i];
        uint32_t plain, trapped, before, after;
        const struct bs_mmio_event *e;

        plain = f->fn(FORM_BASE);
        (void)bs_mmio_trap_enable(g_log, LOG_CAPACITY);
        before = bs_mmio_trap_stats()->loads;
        trapped = f->fn(FORM_BASE);
        after = bs_mmio_trap_stats()->loads;
        bs_mmio_trap_disable();
        e = &g_log[after - 1u];
        test_check_eq(trapped, plain, f->name);
        test_check(after == before + 1u &&
                       e->address == FORM_BASE + f->offset &&
                       e->size == f->size,
                   "  ...logged once with address and size");
    }
}

static void test_stores(void)
{
    uint32_t v;
    (void)bs_mmio_trap_enable(g_log, LOG_CAPACITY);
    st16_word_imm(FORM_BASE, 0x11223344u);
    st16_byte_imm(FORM_BASE, 0xABu);
    bs_mmio_trap_disable();
    v = *(volatile uint32_t *)GPIOB_AFRH_ADDR;
    test_check_eq(v, 0x112233ABu, "trapped word + byte stores reached GPIOB_AFRH");
    *(volatile uint32_t *)GPIOB_AFRH_ADDR = PATTERN;
}

static void test_interrupts(uint32_t prio, const char *name)
{
    uint32_t i, ok = 1u, before, after, isr;
    g_isr_reads = 0u;
    g_isr_bad = 0u;
    SCB_SHPR3 = (SCB_SHPR3 & 0x00FFFFFFu) | (prio << 24);
    (void)bs_mmio_trap_enable(g_log, LOG_CAPACITY);
    before = bs_mmio_trap_stats()->loads;
    SYST_RVR = 1600u - 1u; /* 10 kHz */
    SYST_CVR = 0u;
    SYST_CSR = 0x7u;
    for (i = 0; i < 2000u; ++i) {
        if (*(volatile uint32_t *)GPIOB_AFRH_ADDR != PATTERN) {
            ok = 0u;
        }
    }
    SYST_CSR = 0u;
    after = bs_mmio_trap_stats()->loads;
    isr = g_isr_reads;
    bs_mmio_trap_disable();
    test_check(ok && isr > 0u && g_isr_bad == 0u, name);
    test_check_eq(after - before, 2000u + isr, "  ...every load captured");
}

static void measure(void)
{
    uint32_t i, t0, trapped, plain, sum = 0u;

    DEMCR |= 1u << 24; /* TRCENA */
    DWT_CYCCNT = 0u;
    DWT_CTRL |= 1u;    /* CYCCNTENA */

    t0 = DWT_CYCCNT;
    for (i = 0; i < 1000u; ++i) {
        sum += *(volatile uint32_t *)GPIOB_AFRH_ADDR;
    }
    plain = DWT_CYCCNT - t0;

    (void)bs_mmio_trap_enable(g_log, LOG_CAPACITY);
    t0 = DWT_CYCCNT;
    for (i = 0; i < 1000u; ++i) {
        sum += *(volatile uint32_t *)GPIOB_AFRH_ADDR;
    }
    trapped = DWT_CYCCNT - t0;
    bs_mmio_trap_disable();
    (void)sum;

    test_puts("MEASURE cycles_per_1000_loads trapped=");
    test_put_u32(trapped);
    test_puts(" plain=");
    test_put_u32(plain);
    test_puts(" extra_cycles_per_trapped_load=");
    test_put_u32((trapped - plain + 500u) / 1000u);
    test_puts("\n");
    test_check(trapped > plain && DWT_CYCCNT != 0u, "DWT cycle counter works");
}

int main(void)
{
    uint32_t saved_afrh;

    test_uart_init();
    test_puts("\nBACKSTEP-M1 NUCLEO-F411RE MPU trap test\n");

    NT_RCC_AHB1ENR |= 1u << 1; /* GPIOB clock */
    (void)NT_RCC_AHB1ENR;
    saved_afrh = *(volatile uint32_t *)GPIOB_AFRH_ADDR;
    *(volatile uint32_t *)GPIOB_AFRH_ADDR = PATTERN;
    test_check_eq(*(volatile uint32_t *)GPIOB_AFRH_ADDR, PATTERN,
                  "GPIOB_AFRH holds test pattern");
    test_check_eq((uint32_t)bs_mmio_trap_enable(g_log, LOG_CAPACITY), 0u,
                  "MPU present, trap enabled");
    bs_mmio_trap_disable();

    test_forms();
    test_stores();
    test_interrupts(0x80u, "SysTick priority 0x80: loads correct");
    test_interrupts(0x00u, "SysTick priority 0 (HardFault path): loads correct");

    (void)bs_mmio_trap_enable(g_log, LOG_CAPACITY);
    test_puts("printing with every USART2 access trapped...\n");
    test_check(bs_mmio_trap_stats()->loads > 40u,
               "console works while trapped (SR polls captured)");
    bs_mmio_trap_disable();

    measure();
    *(volatile uint32_t *)GPIOB_AFRH_ADDR = saved_afrh;
    test_finish();
    return 0;
}
