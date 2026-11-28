/*
 * Thumb / Thumb-2 load/store decoder. See thumb_ldst.h.
 *
 * Encodings follow the ARMv7-M Architecture Reference Manual (DDI 0403),
 * section A5.2 (16-bit) and A5.3.7-A5.3.10 (32-bit load/store single).
 *
 * SPDX-License-Identifier: MIT
 */
#include "bs_no_fpu.h"
#include "thumb_ldst.h"

#define REG_SP 13u
#define REG_PC 15u

static void set_imm(struct bs_ldst *o, uint8_t length, uint8_t is_load,
                    uint8_t size, uint8_t sign, uint8_t rt, uint8_t rn,
                    int32_t imm, uint8_t mode)
{
    o->length = length;
    o->is_load = is_load;
    o->size = size;
    o->sign_extend = sign;
    o->rt = rt;
    o->rn = rn;
    o->rm = 0;
    o->shift = 0;
    o->offset_is_register = 0;
    o->mode = mode;
    o->imm = imm;
}

static enum bs_ldst_status decode16(uint16_t hw, struct bs_ldst *o)
{
    uint8_t rt = (uint8_t)(hw & 7u);
    uint8_t rn = (uint8_t)((hw >> 3) & 7u);
    uint8_t imm5 = (uint8_t)((hw >> 6) & 0x1Fu);

    switch (hw >> 11) {
    case 0x0Cu: /* 01100 STR  Rt,[Rn,#imm5*4] */
    case 0x0Du: /* 01101 LDR */
        set_imm(o, 2, (uint8_t)((hw >> 11) & 1u), 4, 0, rt, rn,
                (int32_t)imm5 * 4, BS_ADDR_OFFSET);
        return BS_LDST_OK;
    case 0x0Eu: /* 01110 STRB Rt,[Rn,#imm5] */
    case 0x0Fu: /* 01111 LDRB */
        set_imm(o, 2, (uint8_t)((hw >> 11) & 1u), 1, 0, rt, rn,
                (int32_t)imm5, BS_ADDR_OFFSET);
        return BS_LDST_OK;
    case 0x10u: /* 10000 STRH Rt,[Rn,#imm5*2] */
    case 0x11u: /* 10001 LDRH */
        set_imm(o, 2, (uint8_t)((hw >> 11) & 1u), 2, 0, rt, rn,
                (int32_t)imm5 * 2, BS_ADDR_OFFSET);
        return BS_LDST_OK;
    case 0x0Au: /* 0101 opB Rm Rn Rt: register offset */
    case 0x0Bu: {
        static const uint8_t k_load[8] = {0, 0, 0, 1, 1, 1, 1, 1};
        static const uint8_t k_size[8] = {4, 2, 1, 1, 4, 2, 1, 2};
        static const uint8_t k_sign[8] = {0, 0, 0, 1, 0, 0, 0, 1};
        unsigned opb = (hw >> 9) & 7u;
        set_imm(o, 2, k_load[opb], k_size[opb], k_sign[opb], rt, rn, 0,
                BS_ADDR_OFFSET);
        o->offset_is_register = 1;
        o->rm = (uint8_t)((hw >> 6) & 7u);
        return BS_LDST_OK;
    }
    case 0x09u: /* 01001 LDR literal: PC-relative, never a peripheral */
    case 0x12u: /* 10010 STR  Rt,[SP,#imm8*4] */
    case 0x13u: /* 10011 LDR  Rt,[SP,#imm8*4] */
        return BS_LDST_PC_OR_SP;
    case 0x18u: /* 11000 STM */
    case 0x19u: /* 11001 LDM */
        return BS_LDST_UNSUPPORTED;
    default:
        return BS_LDST_NOT_LDST;
    }
}

static enum bs_ldst_status decode32(uint16_t hw1, uint16_t hw2,
                                    struct bs_ldst *o)
{
    uint8_t rn = (uint8_t)(hw1 & 0xFu);
    uint8_t rt = (uint8_t)(hw2 >> 12);
    uint8_t sign = (uint8_t)((hw1 >> 8) & 1u);
    uint8_t is_load = (uint8_t)((hw1 >> 4) & 1u);
    unsigned size_bits = (hw1 >> 5) & 3u;

    if ((hw1 & 0xFE00u) == 0xE800u) {
        /* 1110 100x: LDM/STM, LDRD/STRD, LDREX/STREX, TBB/TBH */
        return BS_LDST_UNSUPPORTED;
    }
    if ((hw1 & 0xFE00u) != 0xF800u) {
        /* Not "load/store single data item" (1111 100x). */
        return BS_LDST_NOT_LDST;
    }
    if (size_bits == 3u || (sign && !is_load) || (sign && size_bits == 2u)) {
        return BS_LDST_NOT_LDST; /* undefined / other instruction class */
    }
    if (rn == REG_PC) {
        return BS_LDST_PC_OR_SP; /* literal load */
    }
    if (rt == REG_PC && is_load && size_bits != 2u) {
        return BS_LDST_NOT_LDST; /* PLD / PLI memory hints */
    }
    if (rt == REG_PC || rt == REG_SP || rn == REG_SP) {
        return BS_LDST_PC_OR_SP;
    }

    o->length = 4;
    o->is_load = is_load;
    o->size = (uint8_t)(1u << size_bits);
    o->sign_extend = sign;
    o->rt = rt;
    o->rn = rn;
    o->rm = 0;
    o->shift = 0;
    o->offset_is_register = 0;
    o->mode = BS_ADDR_OFFSET;

    if (hw1 & 0x0080u) {
        /* T3-style: [Rn, #imm12], positive offset, no writeback. */
        o->imm = (int32_t)(hw2 & 0x0FFFu);
        return BS_LDST_OK;
    }
    if ((hw2 & 0x0FC0u) == 0x0000u) {
        /* Register offset: [Rn, Rm, LSL #imm2]. */
        uint8_t rm = (uint8_t)(hw2 & 0xFu);
        if (rm == REG_SP || rm == REG_PC) {
            return BS_LDST_UNSUPPORTED; /* UNPREDICTABLE */
        }
        o->offset_is_register = 1;
        o->rm = rm;
        o->shift = (uint8_t)((hw2 >> 4) & 3u);
        o->imm = 0;
        return BS_LDST_OK;
    }
    if (hw2 & 0x0800u) {
        /* 1PUW imm8 forms (including 1110 = unprivileged LDRT/STRT). */
        unsigned p = (hw2 >> 10) & 1u;
        unsigned u = (hw2 >> 9) & 1u;
        unsigned w = (hw2 >> 8) & 1u;
        int32_t imm8 = (int32_t)(hw2 & 0xFFu);
        o->imm = u ? imm8 : -imm8;
        if (p && !w) {
            o->mode = BS_ADDR_OFFSET;
        } else if (p && w) {
            o->mode = BS_ADDR_PRE_INDEX;
        } else if (!p && w) {
            o->mode = BS_ADDR_POST_INDEX;
        } else {
            return BS_LDST_NOT_LDST; /* P=0, W=0 is undefined */
        }
        if (o->mode != BS_ADDR_OFFSET && rn == rt) {
            return BS_LDST_UNSUPPORTED; /* UNPREDICTABLE writeback */
        }
        return BS_LDST_OK;
    }
    return BS_LDST_NOT_LDST;
}

enum bs_ldst_status bs_decode_ldst(uint16_t hw1, uint16_t hw2,
                                   struct bs_ldst *out)
{
    if (bs_thumb_is_32bit(hw1)) {
        return decode32(hw1, hw2, out);
    }
    return decode16(hw1, out);
}

void bs_ldst_address(const struct bs_ldst *insn, uint32_t rn_value,
                     uint32_t rm_value, uint32_t *address, uint32_t *new_rn)
{
    uint32_t offset = insn->offset_is_register
                          ? (rm_value << insn->shift)
                          : (uint32_t)insn->imm;
    uint32_t offset_addr = rn_value + offset;

    switch (insn->mode) {
    case BS_ADDR_PRE_INDEX:
        *address = offset_addr;
        *new_rn = offset_addr;
        break;
    case BS_ADDR_POST_INDEX:
        *address = rn_value;
        *new_rn = offset_addr;
        break;
    default:
        *address = offset_addr;
        *new_rn = rn_value;
        break;
    }
}

uint32_t bs_xpsr_it_advance(uint32_t xpsr)
{
    /* ITSTATE[7:0] = xPSR[15:10] : xPSR[26:25] */
    uint32_t it = ((xpsr >> 8) & 0xFCu) | ((xpsr >> 25) & 0x3u);

    if ((it & 0x7u) == 0u) {
        it = 0u;
    } else {
        it = (it & 0xE0u) | ((it << 1) & 0x1Fu);
    }
    xpsr &= ~((0x3Fu << 10) | (0x3u << 25));
    xpsr |= ((it & 0xFCu) << 8) | ((it & 0x3u) << 25);
    return xpsr;
}
