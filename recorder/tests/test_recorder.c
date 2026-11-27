/*
 * Host-side unit tests for the recorder library.
 *
 * Built with the host C compiler (not the ARM toolchain) so the logic is
 * checked on every CI run. The CRC vectors below are shared with the Rust
 * tests in host/crates/backstep-core/src/crc32.rs; both sides must agree.
 *
 * SPDX-License-Identifier: MIT
 */
#include <stdio.h>
#include <string.h>

#include "backstep.h"

static int g_failures;
static int g_checks;

#define CHECK_EQ_U32(actual, expected)                                        \
    do {                                                                      \
        unsigned long a_ = (unsigned long)(actual);                           \
        unsigned long e_ = (unsigned long)(expected);                         \
        g_checks++;                                                           \
        if (a_ != e_) {                                                       \
            g_failures++;                                                     \
            printf("FAIL %s:%d: %s = 0x%08lX, expected 0x%08lX\n", __FILE__, \
                   __LINE__, #actual, a_, e_);                                \
        }                                                                     \
    } while (0)

struct crc_vector {
    const char *input;
    unsigned long expected;
};

/* Standard CRC-32/ISO-HDLC check values. */
static const struct crc_vector k_vectors[] = {
    {"", 0x00000000ul},
    {"a", 0xE8B7BE43ul},
    {"abc", 0x352441C2ul},
    {"123456789", 0xCBF43926ul},
    {"message digest", 0x20159D7Ful},
    {"The quick brown fox jumps over the lazy dog", 0x414FA339ul},
};

static void test_version(void)
{
    char buf[32];
    unsigned long v = bs_version();

    CHECK_EQ_U32(v, 0x000001ul);
    (void)snprintf(buf, sizeof buf, "%d.%d.%d", BACKSTEP_VERSION_MAJOR,
                   BACKSTEP_VERSION_MINOR, BACKSTEP_VERSION_PATCH);
    g_checks++;
    if (strcmp(buf, BACKSTEP_VERSION_STRING) != 0) {
        g_failures++;
        printf("FAIL version string \"%s\" != numeric version \"%s\"\n",
               BACKSTEP_VERSION_STRING, buf);
    }
}

static void test_crc_vectors(void)
{
    size_t i;
    for (i = 0; i < sizeof k_vectors / sizeof k_vectors[0]; ++i) {
        CHECK_EQ_U32(bs_crc32(k_vectors[i].input, strlen(k_vectors[i].input)),
                     k_vectors[i].expected);
    }
}

static void test_crc_incremental_matches_one_shot(void)
{
    const char *msg = "The quick brown fox jumps over the lazy dog";
    size_t len = strlen(msg);
    size_t split;

    for (split = 0; split <= len; ++split) {
        unsigned long crc = bs_crc32_update(0u, msg, split);
        crc = bs_crc32_update((uint32_t)crc, msg + split, len - split);
        CHECK_EQ_U32(crc, 0x414FA339ul);
    }
}

static void test_crc_all_byte_values(void)
{
    unsigned char bytes[256];
    unsigned i;
    for (i = 0; i < 256u; ++i) {
        bytes[i] = (unsigned char)i;
    }
    /* Reference value from the bitwise algorithm (see Rust test). */
    CHECK_EQ_U32(bs_crc32(bytes, sizeof bytes), 0x29058C73ul);
}

int main(void)
{
    test_version();
    test_crc_vectors();
    test_crc_incremental_matches_one_shot();
    test_crc_all_byte_values();

    printf("recorder tests: %d checks, %d failures\n", g_checks, g_failures);
    return g_failures == 0 ? 0 : 1;
}
