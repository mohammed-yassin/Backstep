/*
 * Platform-independent test reporting for Backstep test firmware.
 *
 * Before including this header, define:
 *   static inline void test_putc(char c);            -- console output
 *   static inline void test_platform_finish(int ok); -- end of test run
 * Tests print "PASS <name>" / "FAIL <name>" lines and a final
 * "RESULT PASS" or "RESULT FAIL" line that the host scripts parse.
 *
 * SPDX-License-Identifier: MIT
 */
#ifndef BACKSTEP_TEST_HARNESS_H
#define BACKSTEP_TEST_HARNESS_H

#include <stdint.h>

#define QT_REG32(a) (*(volatile uint32_t *)(a))

static int qt_failures;
static int qt_checks;

static inline void test_puts(const char *s)
{
    while (*s != '\0') {
        test_putc(*s++);
    }
}

static inline void test_put_hex(uint32_t v)
{
    static const char hex[] = "0123456789ABCDEF";
    int shift;
    test_puts("0x");
    for (shift = 28; shift >= 0; shift -= 4) {
        test_putc(hex[(v >> (unsigned)shift) & 0xFu]);
    }
}

static inline void test_put_u32(uint32_t v)
{
    char buf[11];
    int i = 10;
    buf[i] = '\0';
    do {
        buf[--i] = (char)('0' + (v % 10u));
        v /= 10u;
    } while (v != 0u && i > 0);
    test_puts(&buf[i]);
}

static inline void test_check(int ok, const char *name)
{
    qt_checks++;
    test_puts(ok ? "PASS " : "FAIL ");
    test_puts(name);
    test_puts("\n");
    if (!ok) {
        qt_failures++;
    }
}

static inline void test_check_eq(uint32_t got, uint32_t want, const char *name)
{
    test_check(got == want, name);
    if (got != want) {
        test_puts("     got ");
        test_put_hex(got);
        test_puts(" want ");
        test_put_hex(want);
        test_puts("\n");
    }
}

static inline void test_finish(void)
{
    test_puts(qt_failures == 0 ? "RESULT PASS\n" : "RESULT FAIL\n");
    test_platform_finish(qt_failures == 0);
}

#endif /* BACKSTEP_TEST_HARNESS_H */
