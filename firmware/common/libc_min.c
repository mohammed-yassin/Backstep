/*
 * The four memory functions GCC may emit calls to even in freestanding
 * code (struct copies, array initialisers). Byte loops are enough for
 * test firmware. The optimize attribute stops GCC from turning these
 * loops back into calls to themselves.
 *
 * SPDX-License-Identifier: MIT
 */
#include <stddef.h>

void *memcpy(void *dst, const void *src, size_t n);
void *memmove(void *dst, const void *src, size_t n);
void *memset(void *dst, int c, size_t n);
int memcmp(const void *a, const void *b, size_t n);

#define NO_LIBCALL __attribute__((optimize("no-tree-loop-distribute-patterns")))

NO_LIBCALL void *memcpy(void *dst, const void *src, size_t n)
{
    unsigned char *d = dst;
    const unsigned char *s = src;
    while (n-- != 0u) {
        *d++ = *s++;
    }
    return dst;
}

NO_LIBCALL void *memmove(void *dst, const void *src, size_t n)
{
    unsigned char *d = dst;
    const unsigned char *s = src;
    if (d < s) {
        while (n-- != 0u) {
            *d++ = *s++;
        }
    } else {
        while (n-- != 0u) {
            d[n] = s[n];
        }
    }
    return dst;
}

NO_LIBCALL void *memset(void *dst, int c, size_t n)
{
    unsigned char *d = dst;
    while (n-- != 0u) {
        *d++ = (unsigned char)c;
    }
    return dst;
}

NO_LIBCALL int memcmp(const void *a, const void *b, size_t n)
{
    const unsigned char *x = a;
    const unsigned char *y = b;
    for (; n != 0u; --n, ++x, ++y) {
        if (*x != *y) {
            return *x < *y ? -1 : 1;
        }
    }
    return 0;
}
