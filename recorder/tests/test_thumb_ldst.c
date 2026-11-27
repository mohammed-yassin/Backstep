/*
 * Host unit tests for the Thumb load/store decoder (src/thumb_ldst.c).
 *
 * 1. ~1000 vectors assembled by the GNU assembler (gen_ldst_vectors.py):
 *    every decoded field must match what the assembly text says.
 * 2. Address/writeback arithmetic for every addressing mode.
 * 3. IT-state advance against the architecture's ITAdvance() rules.
 * 4. Robustness: all 65536 16-bit halfwords and 2M random 32-bit
 *    encodings decode without crashing and with consistent fields.
 *
 * SPDX-License-Identifier: MIT
 */
#include <stdio.h>
#include <string.h>

#include "../src/thumb_ldst.h"

struct ldst_vector {
    const char *asm_text;
    uint16_t hw1, hw2;
    int status, length, is_load, size, sign, rt, rn, rm, shift, regoff, mode;
    int imm;
};

#include "ldst_vectors.h"

static int g_checks;
static int g_failures;

#define EXPECT(cond, ...)                                                    \
    do {                                                                     \
        g_checks++;                                                          \
        if (!(cond)) {                                                       \
            g_failures++;                                                    \
            printf("FAIL %s:%d: ", __FILE__, __LINE__);                      \
            printf(__VA_ARGS__);                                             \
            printf("\n");                                                    \
        }                                                                    \
    } while (0)

static void test_assembler_vectors(void)
{
    size_t i;
    size_t n = sizeof k_ldst_vectors / sizeof k_ldst_vectors[0];

    for (i = 0; i < n; ++i) {
        const struct ldst_vector *v = &k_ldst_vectors[i];
        struct bs_ldst d;
        enum bs_ldst_status st;

        memset(&d, 0xA5, sizeof d);
        st = bs_decode_ldst(v->hw1, v->hw2, &d);
        EXPECT((int)st == v->status, "%s (%04X %04X): status %d, want %d",
               v->asm_text, v->hw1, v->hw2, (int)st, v->status);
        if (st != BS_LDST_OK || v->status != BS_LDST_OK) {
            continue;
        }
        EXPECT(d.length == v->length && d.is_load == v->is_load &&
                   d.size == v->size && d.sign_extend == v->sign &&
                   d.rt == v->rt && d.rn == v->rn &&
                   d.offset_is_register == v->regoff && d.mode == v->mode,
               "%s: len %u load %u size %u sign %u rt %u rn %u reg %u mode %u",
               v->asm_text, d.length, d.is_load, d.size, d.sign_extend, d.rt,
               d.rn, d.offset_is_register, d.mode);
        if (v->regoff) {
            EXPECT(d.rm == v->rm && d.shift == v->shift, "%s: rm %u shift %u",
                   v->asm_text, d.rm, d.shift);
        } else {
            EXPECT(d.imm == v->imm, "%s: imm %ld, want %d", v->asm_text,
                   (long)d.imm, v->imm);
        }
    }
    printf("decoder: %zu assembler vectors checked\n", n);
}

static void test_address_modes(void)
{
    struct bs_ldst d;
    uint32_t addr, rn;

    memset(&d, 0, sizeof d);
    d.mode = BS_ADDR_OFFSET;
    d.imm = -8;
    bs_ldst_address(&d, 0x40000010u, 0, &addr, &rn);
    EXPECT(addr == 0x40000008u && rn == 0x40000010u, "offset: %08lX %08lX",
           (unsigned long)addr, (unsigned long)rn);

    d.mode = BS_ADDR_PRE_INDEX;
    d.imm = 4;
    bs_ldst_address(&d, 0x40000010u, 0, &addr, &rn);
    EXPECT(addr == 0x40000014u && rn == 0x40000014u, "pre: %08lX %08lX",
           (unsigned long)addr, (unsigned long)rn);

    d.mode = BS_ADDR_POST_INDEX;
    d.imm = -4;
    bs_ldst_address(&d, 0x40000010u, 0, &addr, &rn);
    EXPECT(addr == 0x40000010u && rn == 0x4000000Cu, "post: %08lX %08lX",
           (unsigned long)addr, (unsigned long)rn);

    d.mode = BS_ADDR_OFFSET;
    d.offset_is_register = 1;
    d.shift = 2;
    bs_ldst_address(&d, 0x40020000u, 5u, &addr, &rn);
    EXPECT(addr == 0x40020014u && rn == 0x40020000u, "reg lsl2: %08lX",
           (unsigned long)addr);
}

/* Reference ITAdvance on the 8-bit ITSTATE value. */
static unsigned it_advance_ref(unsigned it)
{
    if ((it & 7u) == 0u) {
        return 0u;
    }
    return (it & 0xE0u) | ((it << 1) & 0x1Fu);
}

static void test_it_advance(void)
{
    unsigned it;
    for (it = 0; it < 256u; ++it) {
        /* Place ITSTATE in xPSR and surround it with other set bits that
         * must survive untouched. */
        uint32_t other = 0xF9000000u & ~(3u << 25); /* N Z C V Q, T bit */
        uint32_t xpsr = other | 0x1FFu /* exception number */ |
                        ((it & 0xFCu) << 8) | ((it & 3u) << 25);
        uint32_t got = bs_xpsr_it_advance(xpsr);
        unsigned want = it_advance_ref(it);
        unsigned got_it = ((got >> 8) & 0xFCu) | ((got >> 25) & 3u);
        EXPECT(got_it == want, "IT %02X -> %02X, want %02X", it, got_it, want);
        EXPECT((got & ~((0x3Fu << 10) | (3u << 25))) == (other | 0x1FFu),
               "IT %02X: other xPSR bits changed", it);
    }
}

static void check_consistent(uint16_t hw1, uint16_t hw2)
{
    struct bs_ldst d;
    enum bs_ldst_status st = bs_decode_ldst(hw1, hw2, &d);

    if (st != BS_LDST_OK) {
        return;
    }
    EXPECT(d.length == (bs_thumb_is_32bit(hw1) ? 4u : 2u), "%04X %04X len",
           hw1, hw2);
    EXPECT(d.size == 1u || d.size == 2u || d.size == 4u, "%04X %04X size",
           hw1, hw2);
    EXPECT(d.rt <= 14u && d.rt != 13u && d.rn <= 14u && d.rn != 13u,
           "%04X %04X regs rt %u rn %u", hw1, hw2, d.rt, d.rn);
    EXPECT(!(d.sign_extend && (!d.is_load || d.size == 4u)), "%04X %04X sign",
           hw1, hw2);
    EXPECT(d.mode <= BS_ADDR_POST_INDEX, "%04X %04X mode", hw1, hw2);
}

static void test_robustness(void)
{
    uint32_t hw;
    uint32_t state = 0xC0FFEEu;
    long i;

    for (hw = 0; hw <= 0xFFFFu; ++hw) {
        check_consistent((uint16_t)hw, 0u);
    }
    for (i = 0; i < 2000000L; ++i) {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        /* Bias half of the samples into the load/store single space. */
        if (i & 1) {
            check_consistent((uint16_t)(0xF800u | ((state >> 16) & 0x1FFu)),
                             (uint16_t)state);
        } else {
            check_consistent((uint16_t)(state >> 16), (uint16_t)state);
        }
    }
}

int main(void)
{
    test_assembler_vectors();
    test_address_modes();
    test_it_advance();
    test_robustness();
    printf("thumb_ldst tests: %d checks, %d failures\n", g_checks, g_failures);
    return g_failures == 0 ? 0 : 1;
}
