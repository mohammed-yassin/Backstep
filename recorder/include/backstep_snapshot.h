/*
 * Backstep starting-state snapshot (EXPERIMENTAL, milestone M1).
 *
 * Replay starts from a snapshot of everything the CPU will need: core
 * registers, the System Control Space state that changes how exceptions
 * behave (vector table, priorities, enabled interrupts, MPU, SysTick
 * configuration), and the contents of RAM. Peripheral registers are not
 * needed: replay never models peripherals, it replays recorded reads.
 *
 * bs_snapshot_take() works like setjmp(): it returns 0 after recording the
 * CPU state and calling save(), and 1 when a machine restored from that
 * snapshot resumes at the same point. save() runs on stack below the
 * caller's SP (memory the program considers dead), so the program's RAM
 * is unchanged while save() copies it out, as long as save() writes only
 * to memory outside the saved regions (e.g. the .backstep section).
 * See docs/adr/0005-starting-state.md.
 *
 * SPDX-License-Identifier: MIT
 */
#ifndef BACKSTEP_SNAPSHOT_H
#define BACKSTEP_SNAPSHOT_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

#define BS_SNAPSHOT_MAGIC 0x504E5342u /* "BSNP" little-endian */
#define BS_SNAPSHOT_VERSION 1u
#define BS_SNAPSHOT_MAX_REGIONS 4u
/* Dead stack below the caller's SP that bs_snapshot_take() zeroes after
 * save() returns (save() must use no more). */
#define BS_SNAPSHOT_SAVE_BYTES 1024u

/* CPU state at the return from bs_snapshot_capture(). Field order is part
 * of the file format. */
struct bs_cpu_state {
    uint32_t r[13];      /* r0-r12; r0-r3 and r12 are scratch at a call */
    uint32_t sp;         /* active stack pointer at the call */
    uint32_t lr;
    uint32_t pc;         /* resume address (the return address) */
    uint32_t xpsr;
    uint32_t msp;
    uint32_t psp;
    uint32_t primask;
    uint32_t basepri;
    uint32_t faultmask;
    uint32_t control;
};

/* System Control Space registers that affect execution. */
struct bs_scs_state {
    uint32_t vtor, aircr, ccr, shpr[3], shcsr;
    uint32_t nvic_iser[8];
    uint8_t nvic_ipr[240];
    uint32_t syst_csr, syst_rvr;
    uint32_t mpu_ctrl, mpu_rbar[8], mpu_rasr[8];
    uint32_t cpacr;
};

struct bs_region {
    uint32_t address;
    uint32_t length;
};

/* File header; followed by the bytes of each region, in order. */
struct bs_snapshot_header {
    uint32_t magic;
    uint32_t version;
    uint32_t header_bytes;
    uint32_t region_count;
    struct bs_cpu_state cpu;
    struct bs_scs_state scs;
    struct bs_region regions[BS_SNAPSHOT_MAX_REGIONS];
    uint32_t regions_crc32; /* CRC-32 of all region bytes, in order */
};

/* Floating-point state at a bs_snapshot_take() (M7). valid is 1 when the
 * FPU was accessible there (CPACR grants the current mode access to
 * cp10), and the other fields are then filled; otherwise everything is 0
 * and the FPU was not touched. Field order is part of the file format. */
struct bs_fp_state {
    uint32_t valid;
    uint32_t s[32];  /* S0-S31 */
    uint32_t fpscr;
    uint32_t fpccr;  /* FPCCR, FPCAR, FPDSCR (SCS registers) */
    uint32_t fpcar;
    uint32_t fpdscr;
};

/* Where each bs_snapshot_take() leaves the floating-point state it found
 * (in .backstep, outside the program's RAM). Reading it executes FP
 * instructions, which on a core with FPCCR.ASPEN set and CONTROL.FPCA
 * clear first create a new floating-point context (FPSCR = FPDSCR,
 * FPCA = 1); the recorded FPSCR and CONTROL are those after that. */
extern struct bs_fp_state bs_snapshot_fp;

/* Record the caller's r4-r11, SP, return address and special registers
 * into cpu, and the floating-point state into bs_snapshot_fp, then call
 * save(cpu). Returns 0, and 1 in a machine restored from the snapshot
 * (which resumes with r0 = 1 at the return address). On the way back
 * S0-S31 and FPSCR are reloaded from bs_snapshot_fp, so the returning
 * state depends only on what was recorded. */
int bs_snapshot_take(struct bs_cpu_state *cpu,
                     void (*save)(const struct bs_cpu_state *cpu));

/* Fill hdr (CPU from cpu, SCS read live, regions as given, CRC-32). */
void bs_snapshot_header_build(struct bs_snapshot_header *hdr,
                              const struct bs_cpu_state *cpu,
                              const struct bs_region *regions,
                              uint32_t region_count);

/*
 * Resume a machine from a snapshot (used to prove snapshots are complete;
 * the replay engine loads snapshots into its emulator directly). The
 * caller (a debugger) has loaded the RAM regions and copied the header to
 * hdr, set SP to any scratch stack below hdr->cpu.sp, and jumped here
 * with r0 = hdr. Restores the System Control Space, then the CPU
 * registers, and continues at hdr->cpu.pc with r0 = 1. Never returns.
 * The floating-point state is not restored (the test that uses this is
 * built soft-float).
 */
void bs_snapshot_resume(const struct bs_snapshot_header *hdr)
    __attribute__((noreturn));

/* A header buffer outside the program's RAM for the debugger to fill. */
extern struct bs_snapshot_header bs_resume_header;

#ifdef __cplusplus
}
#endif

#endif /* BACKSTEP_SNAPSHOT_H */
