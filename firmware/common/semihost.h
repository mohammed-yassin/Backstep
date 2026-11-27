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
