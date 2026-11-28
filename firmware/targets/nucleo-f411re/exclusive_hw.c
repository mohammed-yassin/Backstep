/*
 * M2 hardware measurement: how the STM32F411's local exclusive monitor
 * decides whether a STREX succeeds.
 *
 * The architecture leaves parts of this IMPLEMENTATION DEFINED, and the
 * replay emulator must decide every STREX exactly as the recorded chip
 * did. Two results are architecturally required and are checked:
 *   - LDREX A, STREX A succeeds;
 *   - CLREX between them makes the STREX fail.
 * The rest are measured and printed as "MEASURE excl <case> status=<0|1>"
 * (0 = the store happened, 1 = it did not):
 *   other_address  LDREX A, STREX B (another word)
 *   other_size     LDREX A, STREXB A
 *   plain_store    LDREX A, STR A, STREX A
 *   exception      LDREX A, PendSV taken and returned, STREX A
 * The emulator's Cpu::exclusive_tags_address is set from these results.
 * Run scripts/hil/m2_nucleo_f411re.py to collect them.
 *
 * SPDX-License-Identifier: MIT
 */
#include <stdint.h>

#include "nucleo_test.h"
#include "vectors.h"

#define SCB_ICSR NT_REG32(0xE000ED04u)

static volatile uint32_t words[4];
static volatile uint32_t pendsv_count;

void PendSV_Handler(void)
{
    pendsv_count++;
}

/* LDREX a ; [between] ; STREX status, value, b. Returns the status. */
#define EXCL_CASE(between, a, b)                                              \
    __extension__({                                                           \
        uint32_t status_, tmp_;                                               \
        __asm__ volatile("ldrex %[t], [%[pa]]\n" between                      \
                         "strex %[s], %[v], [%[pb]]\n"                         \
                         : [s] "=&r"(status_), [t] "=&r"(tmp_)                 \
                         : [pa] "r"(a), [pb] "r"(b), [v] "r"(0x5A5A5A5Au)      \
                         : "memory");                                         \
        (void)tmp_;                                                           \
        status_;                                                              \
    })

static void measure(const char *name, uint32_t status)
{
    test_puts("MEASURE excl ");
    test_puts(name);
    test_puts(" status=");
    test_put_u32(status);
    test_puts("\n");
}

int main(void)
{
    volatile uint32_t *a = &words[0];
    volatile uint32_t *b = &words[2];
    uint32_t status;

    test_uart_init();
    test_puts("BACKSTEP-M2 NUCLEO-F411RE exclusive monitor test\n");

    words[0] = 0;
    status = EXCL_CASE("", a, a);
    test_check_eq(status, 0, "ldrex A; strex A succeeds");
    test_check_eq(words[0], 0x5A5A5A5Au, "  ...and stores");

    words[0] = 0;
    status = EXCL_CASE("clrex\n", a, a);
    test_check_eq(status, 1, "ldrex A; clrex; strex A fails");
    test_check_eq(words[0], 0, "  ...and does not store");

    words[2] = 0;
    measure("other_address", EXCL_CASE("", a, b));

    {
        uint32_t s, t;
        __asm__ volatile("ldrex %[t], [%[pa]]\n"
                         "strexb %[s], %[v], [%[pa]]\n"
                         : [s] "=&r"(s), [t] "=&r"(t)
                         : [pa] "r"(a), [v] "r"(0x5Au)
                         : "memory");
        (void)t;
        measure("other_size", s);
    }

    {
        uint32_t s, t;
        __asm__ volatile("ldrex %[t], [%[pa]]\n"
                         "str %[t], [%[pa]]\n"
                         "strex %[s], %[v], [%[pa]]\n"
                         : [s] "=&r"(s), [t] "=&r"(t)
                         : [pa] "r"(a), [v] "r"(0x5A5A5A5Au)
                         : "memory");
        measure("plain_store", s);
    }

    {
        uint32_t s, t;
        uint32_t before = pendsv_count;
        __asm__ volatile("ldrex %[t], [%[pa]]\n"
                         "str %[pend], [%[icsr]]\n"
                         "dsb\n"
                         "isb\n"
                         "strex %[s], %[v], [%[pa]]\n"
                         : [s] "=&r"(s), [t] "=&r"(t)
                         : [pa] "r"(a), [v] "r"(0x5A5A5A5Au),
                           [pend] "r"(1u << 28), [icsr] "r"(&SCB_ICSR)
                         : "memory");
        (void)t;
        test_check_eq(pendsv_count - before, 1, "PendSV taken between ldrex and strex");
        measure("exception", s);
    }

    test_finish();
    return 0;
}
