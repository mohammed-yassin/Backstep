/*
 * Decoder for the Thumb / Thumb-2 load and store instructions that can
 * access a peripheral register.
 *
 * Used by the MPU trap (mpu_trap.c): when a load or store to the peripheral
 * region faults, the handler decodes the faulting instruction with this
 * module, performs the access itself, logs it, and resumes after it.
 *
 * The decoder is pure (no hardware access) so it is unit-tested on the host
 * against encodings produced by the GNU assembler.
 *
 * SPDX-License-Identifier: MIT
 */
#ifndef BACKSTEP_THUMB_LDST_H
#define BACKSTEP_THUMB_LDST_H

#include <stdint.h>

enum bs_ldst_status {
    BS_LDST_OK = 0,
    BS_LDST_NOT_LDST,       /* not a single load/store instruction */
    BS_LDST_UNSUPPORTED,    /* load/store form we do not emulate */
    BS_LDST_PC_OR_SP,       /* base/target is PC or SP (never a peripheral) */
};

enum bs_ldst_base_mode {
    BS_ADDR_OFFSET,         /* address = Rn + offset, Rn unchanged */
    BS_ADDR_PRE_INDEX,      /* address = Rn + offset, then Rn = address */
    BS_ADDR_POST_INDEX,     /* address = Rn, then Rn = Rn + offset */
};

struct bs_ldst {
    uint8_t length;         /* instruction length in bytes: 2 or 4 */
    uint8_t is_load;        /* 1 = load, 0 = store */
    uint8_t size;           /* access size in bytes: 1, 2 or 4 */
    uint8_t sign_extend;    /* loads only: sign-extend byte/halfword */
    uint8_t rt;             /* transfer register */
    uint8_t rn;             /* base register */
    uint8_t rm;             /* offset register (BS_OFFSET_REGISTER only) */
    uint8_t shift;          /* LSL amount applied to Rm (0..3) */
    uint8_t offset_is_register;
    uint8_t mode;           /* enum bs_ldst_base_mode */
    int32_t imm;            /* signed immediate offset (if not register) */
};

/*
 * Decode the instruction whose first halfword is hw1 and second halfword
 * hw2 (hw2 is ignored for 16-bit instructions).
 */
enum bs_ldst_status bs_decode_ldst(uint16_t hw1, uint16_t hw2,
                                   struct bs_ldst *out);

/* 1 if hw1 starts a 32-bit Thumb-2 instruction. */
static inline int bs_thumb_is_32bit(uint16_t hw1)
{
    return (hw1 & 0xF800u) == 0xE800u || (hw1 & 0xF000u) == 0xF000u;
}

/* Effective address and new base value for a decoded instruction. */
void bs_ldst_address(const struct bs_ldst *insn, uint32_t rn_value,
                     uint32_t rm_value, uint32_t *address,
                     uint32_t *new_rn);

/*
 * Advance the IT (If-Then) state held in xPSR after one instruction
 * completes, as the hardware would (ARMv7-M ARM, ITAdvance()).
 */
uint32_t bs_xpsr_it_advance(uint32_t xpsr);

#endif /* BACKSTEP_THUMB_LDST_H */
