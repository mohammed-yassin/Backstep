/*
 * Backstep peripheral-read capture (EXPERIMENTAL, milestone M1 prototype).
 *
 * Captures every load from the Cortex-M peripheral region
 * (0x40000000-0x5FFFFFFF) without recompiling the application, using the
 * Memory Protection Unit:
 *
 *   1. bs_mmio_trap_enable() makes the peripheral region inaccessible.
 *   2. Every peripheral load or store then raises a MemManage fault (or a
 *      HardFault, if it happens at a priority that cannot preempt).
 *   3. The fault handler decodes the instruction, performs the access with
 *      the MPU briefly disabled, records loads, and resumes after it.
 *
 * Requirements and limits (see docs/adr/0003-peripheral-read-capture.md):
 *   - ARMv7-M with an MPU (Cortex-M3/M4/M7). Privileged code only.
 *   - The application must not use the MPU itself while capturing.
 *   - Accesses made from HardFault or NMI handlers are not captured (the
 *     MPU is bypassed there by design).
 *   - LDM/STM/LDRD/STRD/exclusive accesses to peripherals are never
 *     guessed: they call bs_mmio_trap_fatal() and stop the core.
 *
 * The API is not stable yet.
 *
 * SPDX-License-Identifier: MIT
 */
#ifndef BACKSTEP_MMIO_H
#define BACKSTEP_MMIO_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* Dead stack the capture handler zeroes below its stack top (the exception
 * frame, or MSP when the frame is on PSP) before returning; replay writes
 * the same zeros. It must cover the stack the capture code uses (checked
 * by scripts/check-stack-usage.py). The stub stores 480 bytes below its
 * saved registers, then the 40 bytes of those and the alignment pad. */
#define BS_TRAP_STACK_BYTES 520u

#define BS_MMIO_REGION_BASE 0x40000000u
#define BS_MMIO_REGION_SIZE 0x20000000u

/* One captured peripheral load. */
struct bs_mmio_event {
    uint32_t address;
    uint32_t value;    /* zero-extended raw bus value */
    uint32_t pc;       /* address of the load instruction */
    uint8_t size;      /* 1, 2 or 4 bytes */
    uint8_t reserved[3];
};

struct bs_mmio_trap_stats {
    uint32_t loads;        /* loads captured (also those not stored: full log) */
    uint32_t stores;       /* stores performed on the application's behalf */
    uint32_t dropped;      /* loads not stored because the log was full */
    uint32_t unsupported;  /* faults on instructions we refuse to emulate */
};

/*
 * Start capturing. Loads are appended to log[0..capacity-1]; the first
 * capacity loads are kept and later ones counted in stats.dropped.
 * Returns 0 on success, -1 if the core has no usable MPU.
 */
int bs_mmio_trap_enable(struct bs_mmio_event *log, uint32_t capacity);

/*
 * Same, but trap an arbitrary naturally aligned region of 2^size_log2
 * bytes (size_log2 5..31) instead of the standard peripheral region.
 * Used by the test suite to trap a RAM window, and usable for external
 * memory-mapped devices. Returns -1 on a bad region or missing MPU.
 */
int bs_mmio_trap_enable_region(struct bs_mmio_event *log, uint32_t capacity,
                               uint32_t base, uint32_t size_log2);

/* Stop capturing and restore normal peripheral access. */
void bs_mmio_trap_disable(void);

/* Counters since the last bs_mmio_trap_enable(). */
const volatile struct bs_mmio_trap_stats *bs_mmio_trap_stats(void);

/* Reasons passed to bs_mmio_trap_fatal(). */
enum bs_mmio_fatal_reason {
    BS_MMIO_FATAL_UNSUPPORTED_INSN = 1, /* e.g. LDM/LDRD to a peripheral */
    BS_MMIO_FATAL_ADDRESS_MISMATCH = 2, /* decoder vs MMFAR disagree */
    BS_MMIO_FATAL_REAL_FAULT = 3,       /* a fault that is not ours */
};

/*
 * Called when the trap cannot continue safely. The default (weak)
 * implementation executes BKPT so an attached debugger stops at the
 * problem; firmware may override it to log or reset. It must not return.
 */
void bs_mmio_trap_fatal(uint32_t pc, uint32_t reason);

/*
 * Fault handlers. Point MemManage_Handler and HardFault_Handler at these,
 * or call them from your own handlers' first instruction (they are naked
 * and must be entered with the exception stack frame untouched).
 */
void bs_memmanage_handler(void);
void bs_hardfault_handler(void);

/*
 * ---- Compile-time capture (EXPERIMENTAL) ----------------------------
 *
 * Alternative to the MPU trap for code built with Clang and the Backstep
 * pass plugin (tools/clang-mmio-pass), which turns every volatile load
 * into a call to __bs_mmio_load8/16/32(). Cheaper per read, but blind to
 * assembly and precompiled libraries. Events use the same format; "pc" is
 * the return address of the hook call.
 */
void bs_cc_capture_enable(struct bs_mmio_event *log, uint32_t capacity,
                          uint32_t base, uint32_t size);
void bs_cc_capture_disable(void);
const volatile struct bs_mmio_trap_stats *bs_cc_capture_stats(void);

uint8_t __bs_mmio_load8(const volatile uint8_t *p);
uint16_t __bs_mmio_load16(const volatile uint16_t *p);
uint32_t __bs_mmio_load32(const volatile uint32_t *p);

#ifdef __cplusplus
}
#endif

#endif /* BACKSTEP_MMIO_H */
