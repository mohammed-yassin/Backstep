/*
 * M1 test: capture of peripheral reads with the MPU trap, on QEMU.
 *
 * Uses the CMSDK timer 0 RELOAD register (0x40000008) as a peripheral
 * register with a known value, and checks for every load/store form:
 *   - the application sees exactly the value the hardware returns,
 *   - base-register writeback, sign extension and IT blocks behave as on
 *     real hardware,
 *   - every peripheral load is in the log with the right address, size,
 *     value and PC, and stores reach the peripheral,
 *   - captures work from interrupts at any priority (MemManage directly,
 *     or escalated to HardFault at priority 0),
 *   - an unsupported form (LDM) stops the core instead of being guessed.
 * It also measures the cost of a trapped load (run QEMU with -icount).
 *
 * SPDX-License-Identifier: MIT
 */
#include <stdint.h>

#include "backstep_mmio.h"
#include "qemu_test.h"
#include "vectors.h"

#define TIMER0_BASE 0x40000000u
#define RELOAD_ADDR (TIMER0_BASE + 0x08u)
#define PATTERN 0x8A6B4C2Du /* distinct bytes, sign bits set in bytes 1, 3 */

#define SYST_CSR QT_REG32(0xE000E010u)
#define SYST_RVR QT_REG32(0xE000E014u)
#define SYST_CVR QT_REG32(0xE000E018u)
#define SCB_SHPR3 QT_REG32(0xE000ED20u)

/* Assembly helpers (mmio_forms.S). */
uint32_t ld16_word_imm(uint32_t base);
uint32_t ld16_byte_imm(uint32_t base);
uint32_t ld16_half_imm(uint32_t base);
uint32_t ld16_word_reg(uint32_t base);
uint32_t ld16_sbyte_reg(uint32_t base);
uint32_t ld16_shalf_reg(uint32_t base);
uint32_t ld32_high_regs(uint32_t base);
uint32_t ld32_negative(uint32_t base);
uint64_t ld32_pre_index(uint32_t base);
uint64_t ld32_post_index(uint32_t base);
uint32_t ld32_shifted_reg(uint32_t base);
uint32_t ld32_into_lr(uint32_t base);
uint32_t ld32_into_r12(uint32_t base);
uint32_t ld32_sbyte_imm(uint32_t base);
uint64_t it_load_first(uint32_t base, uint32_t cond);
uint64_t it_load_last(uint32_t base, uint32_t cond);
void st16_word_imm(uint32_t base, uint32_t value);
void st16_byte_imm(uint32_t base, uint32_t value);
void st32_half_imm(uint32_t base, uint32_t value);
uint32_t st32_post_index(uint32_t base, uint32_t value);
void ldm_from_peripheral(uint32_t base);

/* The trap's fault handlers take over the MemManage and HardFault vectors. */
__attribute__((naked)) void MemManage_Handler(void)
{
    __asm__ volatile("b bs_memmanage_handler");
}

__attribute__((naked)) void HardFault_Handler(void)
{
    __asm__ volatile("b bs_hardfault_handler");
}

#define LOG_CAPACITY 16384u
static struct bs_mmio_event g_log[LOG_CAPACITY];
static volatile uint32_t g_isr_reads;
static volatile uint32_t g_isr_bad;
static volatile uint32_t g_expect_fatal; /* expected fatal reason, 0 = none */

void SysTick_Handler(void)
{
    /* A peripheral load inside an interrupt. */
    uint32_t v = *(volatile uint32_t *)RELOAD_ADDR;
    g_isr_reads++;
    if (v != PATTERN) {
        g_isr_bad++;
    }
}

void bs_mmio_trap_fatal(uint32_t pc, uint32_t reason)
{
    (void)pc;
    /* Tests never print through the UART here: it would trap again. */
    if (g_expect_fatal != 0u && reason == g_expect_fatal) {
        bs_mmio_trap_disable();
        test_check(1, reason == BS_MMIO_FATAL_REAL_FAULT
                          ? "genuine fault is not mistaken for a trap"
                          : "unsupported LDM stops the core");
        test_finish();
    }
    bs_mmio_trap_disable();
    test_check(0, "unexpected-fatal-trap");
    test_check_eq(reason, 0u, "fatal-reason");
    test_finish();
}

static uint32_t raw_read_reload(void)
{
    /* Only called with the trap disabled. */
    return *(volatile uint32_t *)RELOAD_ADDR;
}

static void set_reload(uint32_t v)
{
    *(volatile uint32_t *)RELOAD_ADDR = v;
}

/*
 * Every load form is run twice on the same address: once with the trap
 * off (the hardware's own answer) and once with it on. The two results,
 * the writeback, and the log entry must all agree with the expected value.
 */
struct form_result {
    uint32_t value;
    uint32_t base;  /* new base for writeback forms, else 0 */
};

typedef struct form_result (*form_fn)(uint32_t base);

#define WRAP1(fn)                                                           \
    static struct form_result w_##fn(uint32_t b)                            \
    {                                                                       \
        struct form_result r = {fn(b), 0u};                                 \
        return r;                                                           \
    }
#define WRAP2(fn)                                                           \
    static struct form_result w_##fn(uint32_t b)                            \
    {                                                                       \
        uint64_t v = fn(b);                                                 \
        struct form_result r = {(uint32_t)v, (uint32_t)(v >> 32)};          \
        return r;                                                           \
    }
WRAP1(ld16_word_imm)
WRAP1(ld16_byte_imm)
WRAP1(ld16_half_imm)
WRAP1(ld16_word_reg)
WRAP1(ld16_sbyte_reg)
WRAP1(ld16_shalf_reg)
WRAP1(ld32_high_regs)
WRAP1(ld32_negative)
WRAP2(ld32_pre_index)
WRAP2(ld32_post_index)
WRAP1(ld32_shifted_reg)
WRAP1(ld32_into_lr)
WRAP1(ld32_into_r12)
WRAP1(ld32_sbyte_imm)

struct form_case {
    const char *name;
    form_fn fn;
    uint32_t arg_offset;   /* base passed = region + arg_offset */
    uint32_t access_offset; /* address accessed = region + access_offset */
    uint8_t size;
    uint8_t word_only_ok;   /* also valid on a device with 32-bit registers */
};

static const struct form_case k_forms[] = {
    {"ldr.n imm5", w_ld16_word_imm, 0, 8, 4, 1},
    {"ldrb.n imm5", w_ld16_byte_imm, 0, 9, 1, 0},
    {"ldrh.n imm5", w_ld16_half_imm, 0, 10, 2, 0},
    {"ldr.n register offset", w_ld16_word_reg, 0, 8, 4, 1},
    {"ldrsb.n register offset (sign)", w_ld16_sbyte_reg, 0, 11, 1, 0},
    {"ldrsh.n register offset (sign)", w_ld16_shalf_reg, 0, 10, 2, 0},
    {"ldr.w r9,[r8] callee-saved regs", w_ld32_high_regs, 0, 8, 4, 1},
    {"ldr negative imm8", w_ld32_negative, 0, 8, 4, 1},
    {"ldr pre-index writeback", w_ld32_pre_index, 0, 8, 4, 1},
    {"ldr post-index writeback", w_ld32_post_index, 8, 8, 4, 1},
    {"ldr.w [Rn,Rm,lsl #2]", w_ld32_shifted_reg, 0, 8, 4, 1},
    {"ldr.w into lr (frame slot)", w_ld32_into_lr, 0, 8, 4, 1},
    {"ldr.w into r12 (frame slot)", w_ld32_into_r12, 0, 8, 4, 1},
    {"ldrsb.w imm12 (sign)", w_ld32_sbyte_imm, 0, 11, 1, 0},
};

#define RAM_WINDOW 0x20200000u      /* 1 MB of SRAM trapped like a device */
#define RAM_WINDOW_LOG2 20u

static struct bs_mmio_event *last_event(void)
{
    uint32_t n = bs_mmio_trap_stats()->loads;
    return (n == 0u || n > LOG_CAPACITY) ? (struct bs_mmio_event *)0
                                         : &g_log[n - 1u];
}

static void run_forms(uint32_t region, uint32_t region_log2, int word_only,
                      const char *where)
{
    unsigned i;
    for (i = 0; i < sizeof k_forms / sizeof k_forms[0]; ++i) {
        const struct form_case *c = &k_forms[i];
        struct form_result plain, trapped;
        struct bs_mmio_event *e;
        uint32_t raw, loads_before;
        int ok;

        if (word_only && !c->word_only_ok) {
            continue;
        }
        bs_mmio_trap_disable();
        plain = c->fn(region + c->arg_offset);
        raw = c->size == 4u ? *(volatile uint32_t *)(region + c->access_offset)
              : c->size == 2u ? *(volatile uint16_t *)(region + c->access_offset)
                              : *(volatile uint8_t *)(region + c->access_offset);

        (void)bs_mmio_trap_enable_region(g_log, LOG_CAPACITY, region,
                                         region_log2);
        loads_before = bs_mmio_trap_stats()->loads;
        trapped = c->fn(region + c->arg_offset);
        e = last_event();
        ok = bs_mmio_trap_stats()->loads == loads_before + 1u && e != 0 &&
             e->address == region + c->access_offset && e->size == c->size &&
             e->value == raw;
        bs_mmio_trap_disable();

        test_puts(where);
        test_check(trapped.value == plain.value && trapped.base == plain.base,
                   c->name);
        if (trapped.value != plain.value || trapped.base != plain.base) {
            test_puts("     trapped ");
            test_put_hex(trapped.value);
            test_puts(" plain ");
            test_put_hex(plain.value);
            test_puts("\n");
        }
        test_puts(where);
        test_check(ok, "  ...logged exactly once with address, size, value");
    }
}

static void test_load_forms(void)
{
    struct form_result r;
    volatile uint32_t *ram = (volatile uint32_t *)RAM_WINDOW;

    ram[2] = PATTERN; /* RAM window +8 */

    /* Expected values, checked once on plain RAM, pin down sign handling. */
    test_check_eq(ld16_sbyte_reg(RAM_WINDOW), 0xFFFFFF8Au, "ram: ldrsb reference");
    test_check_eq(ld16_shalf_reg(RAM_WINDOW), 0xFFFF8A6Bu, "ram: ldrsh reference");

    run_forms(RAM_WINDOW, RAM_WINDOW_LOG2, 0, "ram:   ");
    run_forms(TIMER0_BASE, 29u, 1, "timer: ");

    /* IT blocks (on the timer): taken loads trap, skipped loads must not. */
    (void)bs_mmio_trap_enable(g_log, LOG_CAPACITY);
    {
        uint32_t l0 = bs_mmio_trap_stats()->loads;
        uint64_t a = it_load_first(TIMER0_BASE, 1u);
        uint32_t l1 = bs_mmio_trap_stats()->loads;
        uint64_t b = it_load_first(TIMER0_BASE, 0u);
        uint32_t l2 = bs_mmio_trap_stats()->loads;
        uint64_t c = it_load_last(TIMER0_BASE, 1u);
        uint32_t l3 = bs_mmio_trap_stats()->loads;
        uint64_t d = it_load_last(TIMER0_BASE, 0u);
        uint32_t l4 = bs_mmio_trap_stats()->loads;

        r.value = (uint32_t)a;
        test_check(r.value == PATTERN && (uint32_t)(a >> 32) == 1u &&
                       l1 == l0 + 1u,
                   "IT first slot taken: value, ITSTATE advanced, logged");
        test_check((uint32_t)b == 0u && (uint32_t)(b >> 32) == 2u && l2 == l1,
                   "IT first slot skipped: no access, else-branch ran");
        test_check((uint32_t)c == PATTERN && (uint32_t)(c >> 32) == 5u &&
                       l3 == l2 + 1u,
                   "IT last slot taken: earlier IT instructions intact");
        test_check((uint32_t)d == 0u && (uint32_t)(d >> 32) == 16u && l4 == l3,
                   "IT last slot skipped: no access");
    }
    bs_mmio_trap_disable();
}

static void test_store_forms(void)
{
    volatile uint32_t *ram = (volatile uint32_t *)RAM_WINDOW;
    uint32_t w, b, h, post, base, stores;

    ram[2] = 0u;
    ram[3] = 0x55550000u;
    ram[4] = 0u;
    /* No printing between enable and disable: the counters must only see
     * the stores below. */
    (void)bs_mmio_trap_enable_region(g_log, LOG_CAPACITY, RAM_WINDOW,
                                     RAM_WINDOW_LOG2);
    st16_word_imm(RAM_WINDOW, 0x11223344u);         /* word  -> +8  */
    st16_byte_imm(RAM_WINDOW, 0xFFFFFFABu);         /* byte  -> +8  */
    st32_half_imm(RAM_WINDOW + 4u, 0xFFFFBEEFu);    /* half  -> +12 */
    base = st32_post_index(RAM_WINDOW + 16u, PATTERN); /* word -> +16 */
    stores = bs_mmio_trap_stats()->stores;
    bs_mmio_trap_disable();
    w = ram[2];
    h = ram[3];
    post = ram[4];
    b = w & 0xFFu;

    test_check_eq(w, 0x112233ABu, "str.n then strb.n: word, then one byte");
    test_check_eq(b, 0xABu, "strb.n wrote the low byte");
    test_check_eq(h, 0x5555BEEFu, "strh.w writes one halfword only");
    test_check_eq(post, PATTERN, "str post-index stored at the old base");
    test_check_eq(base, RAM_WINDOW + 20u, "str post-index writeback");
    test_check_eq(stores, 4u, "every store counted");
}

static void test_interrupts(uint32_t priority, const char *name)
{
    const volatile struct bs_mmio_trap_stats *st = bs_mmio_trap_stats();
    uint32_t loads_before, loads_after, isr_reads;
    uint32_t i, ok = 1u;

    g_isr_reads = 0u;
    g_isr_bad = 0u;
    loads_before = st->loads;
    SCB_SHPR3 = (SCB_SHPR3 & 0x00FFFFFFu) | (priority << 24);
    SYST_RVR = 3000u - 1u; /* frequent ticks, mid-way through loads */
    SYST_CVR = 0u;
    SYST_CSR = 0x7u;
    for (i = 0; i < 2000u; ++i) {
        if (*(volatile uint32_t *)RELOAD_ADDR != PATTERN) {
            ok = 0u;
        }
    }
    SYST_CSR = 0u;
    loads_after = st->loads;
    isr_reads = g_isr_reads;
    test_check(ok, name);
    test_check(isr_reads > 0u && g_isr_bad == 0u, "interrupt loads correct");
    test_check_eq(loads_after - loads_before, 2000u + isr_reads,
                  "every main + interrupt load captured");
}

static void measure_overhead(void)
{
    uint32_t i, t0, t_trap, t_plain;
    volatile uint32_t sink = 0u;

    SYST_RVR = 0x00FFFFFFu;
    SYST_CVR = 0u;
    SYST_CSR = 0x5u; /* processor clock, no interrupt */
    while (SYST_CVR == 0u) {
        /* wait for the first reload so the counter is running */
    }

    t0 = SYST_CVR;
    for (i = 0; i < 1000u; ++i) {
        sink += *(volatile uint32_t *)RELOAD_ADDR;
    }
    t_trap = (t0 - SYST_CVR) & 0x00FFFFFFu;

    bs_mmio_trap_disable();
    t0 = SYST_CVR;
    for (i = 0; i < 1000u; ++i) {
        sink += *(volatile uint32_t *)RELOAD_ADDR;
    }
    t_plain = (t0 - SYST_CVR) & 0x00FFFFFFu;
    SYST_CSR = 0u;
    (void)sink;

    /* Under "-icount shift=0" QEMU executes one instruction per virtual
     * nanosecond and the mps2 SysTick runs at 25 MHz: 1 tick = 40
     * instructions. */
    test_puts("MEASURE systicks_per_1000_loads trapped=");
    test_put_u32(t_trap);
    test_puts(" plain=");
    test_put_u32(t_plain);
    test_puts(" => extra_instructions_per_trapped_load=");
    test_put_u32(((t_trap - t_plain) * 40u + 500u) / 1000u);
    test_puts("\n");
    test_check(t_trap > t_plain, "trapped loads cost more than plain loads");
}

/*
 * Run with "-append real-fault": after normal trapped loads (which set and
 * must clear the fault status bits), a genuine MemManage fault (fetching
 * code from the peripheral region) must be reported, never emulated.
 */
static void real_fault_test(void)
{
    void (*bad_fn)(void) = (void (*)(void))(TIMER0_BASE | 1u);
    set_reload(PATTERN);
    (void)bs_mmio_trap_enable(g_log, LOG_CAPACITY);
    (void)ld16_word_imm(TIMER0_BASE);
    (void)ld16_word_imm(TIMER0_BASE);
    g_expect_fatal = BS_MMIO_FATAL_REAL_FAULT;
    bad_fn();
    bs_mmio_trap_disable();
    test_check(0, "genuine fault is not mistaken for a trap");
    test_finish();
}

int main(void)
{
    const volatile struct bs_mmio_trap_stats *st;
    char cmdline[32];

    test_uart_init();
    if (semihost_get_cmdline(cmdline, sizeof cmdline) == 0 &&
        cmdline[0] == 'r') {
        test_puts("Backstep M1 MPU trap test (real-fault mode)\n");
        real_fault_test();
    }
    test_puts("Backstep M1 MPU trap test\n");
    set_reload(PATTERN);
    test_check_eq(raw_read_reload(), PATTERN, "timer RELOAD holds pattern");

    test_check_eq((uint32_t)bs_mmio_trap_enable(g_log, LOG_CAPACITY), 0u,
                  "trap enabled (MPU present)");
    bs_mmio_trap_disable();
    test_check_eq((uint32_t)bs_mmio_trap_enable_region(g_log, 1u, 0x20200100u, 20u),
                  0xFFFFFFFFu, "misaligned region rejected");

    test_load_forms();
    test_store_forms();

    (void)bs_mmio_trap_enable(g_log, LOG_CAPACITY);
    st = bs_mmio_trap_stats();
    test_interrupts(0x80u, "loads correct with SysTick at priority 0x80");
    test_interrupts(0x00u, "loads correct with SysTick at priority 0 (HardFault path)");

    test_check_eq(st->dropped, 0u, "log did not overflow");
    test_check_eq(st->unsupported, 0u, "no unsupported accesses");
    test_check(st->loads > 4000u, "UART output itself was captured too");

    measure_overhead();

    /* Last: an unsupported multi-register load must stop the core. */
    test_check_eq((uint32_t)bs_mmio_trap_enable(g_log, LOG_CAPACITY), 0u,
                  "trap re-enabled");
    g_expect_fatal = BS_MMIO_FATAL_UNSUPPORTED_INSN;
    ldm_from_peripheral(TIMER0_BASE);
    bs_mmio_trap_disable();
    test_check(0, "unsupported-ldm-stops-the-core");
    test_finish();
    return 0;
}
