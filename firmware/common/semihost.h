/*
 * Tiny Arm semihosting helpers used by QEMU-based test firmware to report
 * PASS/FAIL through the emulator's process exit code.
 *
 * Only for emulator targets: on real hardware without a debugger attached,
 * a semihosting BKPT causes a HardFault.
 *
 * SPDX-License-Identifier: MIT
 */
#ifndef BACKSTEP_SEMIHOST_H
#define BACKSTEP_SEMIHOST_H

#include <stdint.h>

#define SEMIHOST_SYS_OPEN 0x01u
#define SEMIHOST_SYS_CLOSE 0x02u
#define SEMIHOST_SYS_WRITE 0x05u
#define SEMIHOST_SYS_GET_CMDLINE 0x15u
#define SEMIHOST_SYS_EXIT 0x18u
#define SEMIHOST_ADP_STOPPED_APPLICATION_EXIT 0x20026u
#define SEMIHOST_ADP_STOPPED_RUNTIME_ERROR 0x20023u

static inline uint32_t semihost_call(uint32_t op, uint32_t arg)
{
    register uint32_t r0 __asm__("r0") = op;
    register uint32_t r1 __asm__("r1") = arg;
    __asm__ volatile("bkpt 0xAB" : "+r"(r0) : "r"(r1) : "memory");
    return r0;
}

/*
 * Read the command line QEMU was given with -append into buf (NUL
 * terminated). Returns 0 on success.
 */
static inline int semihost_get_cmdline(char *buf, uint32_t size)
{
    volatile uint32_t block[2];
    block[0] = (uint32_t)(uintptr_t)buf;
    block[1] = size;
    return semihost_call(SEMIHOST_SYS_GET_CMDLINE, (uint32_t)(uintptr_t)block) == 0u
               ? 0
               : -1;
}

/* Open a host file for binary writing (created/truncated). Returns the
 * handle, or 0xFFFFFFFF on failure. */
static inline uint32_t semihost_open_write(const char *name, uint32_t name_len)
{
    volatile uint32_t block[3];
    block[0] = (uint32_t)(uintptr_t)name;
    block[1] = 5u; /* mode "wb" */
    block[2] = name_len;
    return semihost_call(SEMIHOST_SYS_OPEN, (uint32_t)(uintptr_t)block);
}

/* Write len bytes in one call. Returns 0 if everything was written. */
static inline int semihost_write(uint32_t handle, const void *data, uint32_t len)
{
    volatile uint32_t block[3];
    block[0] = handle;
    block[1] = (uint32_t)(uintptr_t)data;
    block[2] = len;
    return semihost_call(SEMIHOST_SYS_WRITE, (uint32_t)(uintptr_t)block) == 0u ? 0 : -1;
}

static inline void semihost_close(uint32_t handle)
{
    volatile uint32_t block[1];
    block[0] = handle;
    (void)semihost_call(SEMIHOST_SYS_CLOSE, (uint32_t)(uintptr_t)block);
}

/* Write len bytes to a new host file in one call. Returns 0 on success. */
static inline int semihost_write_file(const char *name, uint32_t name_len,
                                      const void *data, uint32_t len)
{
    uint32_t h = semihost_open_write(name, name_len);
    int rc;
    if (h == 0xFFFFFFFFu) {
        return -1;
    }
    rc = semihost_write(h, data, len);
    semihost_close(h);
    return rc;
}

/* Exit the emulator: QEMU returns 0 for success, 1 otherwise. */
static inline void semihost_exit(int success)
{
    (void)semihost_call(SEMIHOST_SYS_EXIT,
                        success ? SEMIHOST_ADP_STOPPED_APPLICATION_EXIT
                                : SEMIHOST_ADP_STOPPED_RUNTIME_ERROR);
    for (;;) {
    }
}

#endif /* BACKSTEP_SEMIHOST_H */
