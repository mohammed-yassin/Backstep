/*
 * Starting-state snapshot (EXPERIMENTAL). See backstep_snapshot.h.
 *
 * SPDX-License-Identifier: MIT
 */
#include "backstep.h"
#include "backstep_snapshot.h"

#define REG32(a) (*(volatile uint32_t *)(a))

/*
 * Naked so no prologue disturbs the registers being recorded. On entry
 * r0 = cpu, r1 = save, lr = return address, and the caller's r4-r11 and SP
 * are live. Offsets follow struct bs_cpu_state.
 */
__attribute__((naked)) int bs_snapshot_take(struct bs_cpu_state *cpu,
                                            void (*save)(const struct bs_cpu_state *))
{
    (void)cpu;
    (void)save;
    __asm__ volatile(
        "mov r12, r1\n\t"            /* keep save() in r12 */
        "add r1, r0, #16\n\t"          /* &cpu->r[4] */
        "stmia r1, {r4-r11}\n\t"
        "mov r1, #0\n\t"
        "str r1, [r0, #0]\n\t"         /* r0-r3, r12: zero (scratch) */
        "str r1, [r0, #4]\n\t"
        "str r1, [r0, #8]\n\t"
        "str r1, [r0, #12]\n\t"
        "str r1, [r0, #48]\n\t"
        "mov r1, sp\n\t"
        "str r1, [r0, #52]\n\t"        /* sp */
        "str lr, [r0, #56]\n\t"        /* lr */
        "str lr, [r0, #60]\n\t"        /* pc = return address */
        "mrs r1, xpsr\n\t"
        "str r1, [r0, #64]\n\t"
        "mrs r1, msp\n\t"
        "str r1, [r0, #68]\n\t"
        "mrs r1, psp\n\t"
        "str r1, [r0, #72]\n\t"
        "mrs r1, primask\n\t"
        "str r1, [r0, #76]\n\t"
        "mrs r1, basepri\n\t"
        "str r1, [r0, #80]\n\t"
        "mrs r1, faultmask\n\t"
        "str r1, [r0, #84]\n\t"
        "mrs r1, control\n\t"
        "str r1, [r0, #88]\n\t"
        /* save(cpu) on the stack below the caller's SP (dead memory). */
        "push {r0, lr}\n\t"
        "blx r12\n\t"
        "pop {r0, lr}\n\t"
        /* Return with a state that depends only on the snapshot, so a
         * recording replays from it exactly: the caller's flags (and GE)
         * as at the call, the scratch registers r1-r3 and r12 zero. */
        "ldr r1, [r0, #64]\n\t"
        "msr apsr_nzcvqg, r1\n\t"
        "mov.w r1, #0\n\t"
        "mov.w r2, #0\n\t"
        "mov.w r3, #0\n\t"
        "mov.w r12, #0\n\t"
        "mov.w r0, #0\n\t"
        "bx lr\n\t");
}

void bs_snapshot_header_build(struct bs_snapshot_header *hdr,
                              const struct bs_cpu_state *cpu,
                              const struct bs_region *regions,
                              uint32_t region_count)
{
    struct bs_scs_state *s = &hdr->scs;
    uint32_t i, crc = 0u;
    uint32_t mpu_regions = (REG32(0xE000ED90u) >> 8) & 0xFFu;

    hdr->magic = BS_SNAPSHOT_MAGIC;
    hdr->version = BS_SNAPSHOT_VERSION;
    hdr->header_bytes = (uint32_t)sizeof *hdr;
    hdr->cpu = *cpu;

    s->vtor = REG32(0xE000ED08u);
    s->aircr = REG32(0xE000ED0Cu);
    s->ccr = REG32(0xE000ED14u);
    s->shpr[0] = REG32(0xE000ED18u);
    s->shpr[1] = REG32(0xE000ED1Cu);
    s->shpr[2] = REG32(0xE000ED20u);
    s->shcsr = REG32(0xE000ED24u);
    for (i = 0; i < 8u; ++i) {
        s->nvic_iser[i] = REG32(0xE000E100u + 4u * i);
    }
    for (i = 0; i < 240u; ++i) {
        s->nvic_ipr[i] = *(volatile uint8_t *)(0xE000E400u + i);
    }
    s->syst_csr = REG32(0xE000E010u);
    s->syst_rvr = REG32(0xE000E014u);
    s->mpu_ctrl = REG32(0xE000ED94u);
    for (i = 0; i < 8u; ++i) {
        if (i < mpu_regions) {
            REG32(0xE000ED98u) = i;
            s->mpu_rbar[i] = REG32(0xE000ED9Cu);
            s->mpu_rasr[i] = REG32(0xE000EDA0u);
        } else {
            s->mpu_rbar[i] = 0u;
            s->mpu_rasr[i] = 0u;
        }
    }
    s->cpacr = REG32(0xE000ED88u);

    if (region_count > BS_SNAPSHOT_MAX_REGIONS) {
        region_count = BS_SNAPSHOT_MAX_REGIONS;
    }
    hdr->region_count = region_count;
    for (i = 0; i < BS_SNAPSHOT_MAX_REGIONS; ++i) {
        hdr->regions[i].address = i < region_count ? regions[i].address : 0u;
        hdr->regions[i].length = i < region_count ? regions[i].length : 0u;
    }
    for (i = 0; i < region_count; ++i) {
        crc = bs_crc32_update(crc, (const void *)(uintptr_t)regions[i].address,
                              regions[i].length);
    }
    hdr->regions_crc32 = crc;
}

struct bs_snapshot_header bs_resume_header __attribute__((section(".backstep")));

void bs_snapshot_restore_scs(const struct bs_snapshot_header *hdr);
__attribute__((noreturn)) void bs_snapshot_jump(const struct bs_cpu_state *cpu);

void bs_snapshot_restore_scs(const struct bs_snapshot_header *hdr)
{
    const struct bs_scs_state *s = &hdr->scs;
    uint32_t i;
    uint32_t mpu_regions = (REG32(0xE000ED90u) >> 8) & 0xFFu;

    REG32(0xE000ED08u) = s->vtor;
    REG32(0xE000ED0Cu) = 0x05FA0000u | (s->aircr & 0x0700u); /* PRIGROUP */
    REG32(0xE000ED14u) = s->ccr;
    REG32(0xE000ED18u) = s->shpr[0];
    REG32(0xE000ED1Cu) = s->shpr[1];
    REG32(0xE000ED20u) = s->shpr[2];
    REG32(0xE000ED24u) = s->shcsr & 0x00070000u; /* fault handler enables */
    for (i = 0; i < 240u; ++i) {
        *(volatile uint8_t *)(0xE000E400u + i) = s->nvic_ipr[i];
    }
    for (i = 0; i < 8u; ++i) {
        REG32(0xE000E180u + 4u * i) = 0xFFFFFFFFu; /* disable all, then */
        REG32(0xE000E100u + 4u * i) = s->nvic_iser[i]; /* enable saved */
    }
    REG32(0xE000ED94u) = 0u;
    for (i = 0; i < 8u && i < mpu_regions; ++i) {
        REG32(0xE000ED98u) = i;
        REG32(0xE000ED9Cu) = s->mpu_rbar[i] & ~0x1Fu;
        REG32(0xE000EDA0u) = s->mpu_rasr[i];
    }
    REG32(0xE000ED94u) = s->mpu_ctrl;
    REG32(0xE000ED88u) = s->cpacr;
    REG32(0xE000E014u) = s->syst_rvr;
    REG32(0xE000E018u) = 0u;
    REG32(0xE000E010u) = s->syst_csr & 0x7u;
    __asm__ volatile("dsb\n\tisb" ::: "memory");
}

/* Load r4-r11, special registers, SP and LR from cpu, then branch to the
 * resume PC with r0 = 1. Offsets follow struct bs_cpu_state. */
__attribute__((naked, noreturn)) void bs_snapshot_jump(const struct bs_cpu_state *cpu)
{
    (void)cpu;
    __asm__ volatile(
        "cpsid i\n\t"
        "ldr r1, [r0, #72]\n\t"   /* psp */
        "msr psp, r1\n\t"
        "ldr r1, [r0, #80]\n\t"   /* basepri */
        "msr basepri, r1\n\t"
        "ldr r1, [r0, #84]\n\t"   /* faultmask */
        "msr faultmask, r1\n\t"
        "ldr r1, [r0, #64]\n\t"   /* xpsr: restore the flags (APSR) */
        "msr apsr_nzcvq, r1\n\t"
        "ldr r1, [r0, #52]\n\t"   /* sp */
        "mov sp, r1\n\t"
        "ldr lr, [r0, #56]\n\t"
        "ldr r12, [r0, #60]\n\t"  /* pc */
        "orr r12, r12, #1\n\t"    /* Thumb state */
        "ldr r2, [r0, #76]\n\t"   /* primask */
        "add r1, r0, #16\n\t"
        "ldmia r1, {r4-r11}\n\t"
        "mov r0, #1\n\t"
        "msr primask, r2\n\t"     /* last: interrupts as at the snapshot */
        "bx r12\n\t");
}

void bs_snapshot_resume(const struct bs_snapshot_header *hdr)
{
    bs_snapshot_restore_scs(hdr);
    bs_snapshot_jump(&hdr->cpu);
}
