/*
 * Hand-written load/store instructions in every addressing form the MPU
 * trap emulates. Writing them in assembly pins the exact encoding (16-bit
 * vs 32-bit, register choice) so each form is tested on purpose.
 *
 * Each function receives a peripheral base address in r0. Loads return the
 * loaded value in r0; forms with writeback also return the new base in r1
 * (a 64-bit return value in C).
 *
 * SPDX-License-Identifier: MIT
 */
    .syntax unified
    .thumb
    .text

    .macro FUNC name
    .global \name
    .type \name, %function
    .thumb_func
\name:
    .endm

/* ---- 16-bit loads -------------------------------------------------- */
FUNC ld16_word_imm          /* ldr  Rt,[Rn,#imm5*4] -> word at base+8 */
    ldr.n   r1, [r0, #8]
    mov     r0, r1
    bx      lr

FUNC ld16_byte_imm          /* ldrb Rt,[Rn,#imm5] -> byte at base+9 */
    ldrb.n  r1, [r0, #9]
    mov     r0, r1
    bx      lr

FUNC ld16_half_imm          /* ldrh Rt,[Rn,#imm5*2] -> half at base+10 */
    ldrh.n  r1, [r0, #10]
    mov     r0, r1
    bx      lr

FUNC ld16_word_reg          /* ldr  Rt,[Rn,Rm] -> word at base+8 */
    movs    r2, #8
    ldr.n   r1, [r0, r2]
    mov     r0, r1
    bx      lr

FUNC ld16_sbyte_reg         /* ldrsb Rt,[Rn,Rm] -> byte at base+11 */
    movs    r2, #11
    ldrsb.n r1, [r0, r2]
    mov     r0, r1
    bx      lr

FUNC ld16_shalf_reg         /* ldrsh Rt,[Rn,Rm] -> half at base+10 */
    movs    r2, #10
    ldrsh.n r1, [r0, r2]
    mov     r0, r1
    bx      lr

/* ---- 32-bit loads -------------------------------------------------- */
FUNC ld32_high_regs         /* ldr.w r9,[r8,#8]: callee-saved registers */
    push    {r8, r9}
    mov     r8, r0
    ldr.w   r9, [r8, #8]
    mov     r0, r9
    pop     {r8, r9}
    bx      lr

FUNC ld32_negative          /* ldr Rt,[Rn,#-8] with Rn = base+16 */
    adds    r0, #16
    ldr     r1, [r0, #-8]
    mov     r0, r1
    bx      lr

FUNC ld32_pre_index         /* ldr Rt,[Rn,#8]! -> r0 value, r1 new base */
    ldr     r2, [r0, #8]!
    mov     r1, r0
    mov     r0, r2
    bx      lr

FUNC ld32_post_index        /* ldr Rt,[Rn],#8 (r0 = base+8) */
    ldr     r2, [r0], #8
    mov     r1, r0
    mov     r0, r2
    bx      lr

FUNC ld32_shifted_reg       /* ldr.w Rt,[Rn,Rm,lsl #2] -> base+8 */
    movs    r2, #2
    ldr.w   r1, [r0, r2, lsl #2]
    mov     r0, r1
    bx      lr

FUNC ld32_into_lr           /* destination LR (stacked-frame slot) */
    push    {lr}
    ldr.w   lr, [r0, #8]
    mov     r0, lr
    pop     {lr}
    bx      lr

FUNC ld32_into_r12          /* destination R12 (stacked-frame slot) */
    ldr.w   r12, [r0, #8]
    mov     r0, r12
    bx      lr

FUNC ld32_sbyte_imm         /* ldrsb.w Rt,[Rn,#11] */
    ldrsb.w r1, [r0, #11]
    mov     r0, r1
    bx      lr

/* ---- IT blocks ------------------------------------------------------ */
/* r0 = base, r1 = condition. Returns r0 = loaded value (or 0),
 * r1 = marker: 1 if condition true, 2 if false. */
FUNC it_load_first
    movs    r2, #0
    movs    r3, #0
    cmp     r1, #0
    itte    ne
    ldrne   r2, [r0, #8]
    addne   r3, r3, #1
    addeq   r3, r3, #2
    mov     r0, r2
    mov     r1, r3
    bx      lr

/* Load in the last slot of a four-instruction IT block; the instructions
 * before it must keep their conditions after the trap advances ITSTATE. */
FUNC it_load_last
    movs    r2, #0
    movs    r3, #0
    cmp     r1, #0
    ittet   ne
    addne   r3, r3, #1
    addne   r3, r3, #4
    addeq   r3, r3, #16
    ldrne   r2, [r0, #8]
    mov     r0, r2
    mov     r1, r3
    bx      lr

/* ---- Stores --------------------------------------------------------- */
FUNC st16_word_imm          /* str.n r1,[r0,#8] */
    str.n   r1, [r0, #8]
    bx      lr

FUNC st16_byte_imm          /* strb.n r1,[r0,#8] */
    strb.n  r1, [r0, #8]
    bx      lr

FUNC st32_half_imm          /* strh.w r1,[r0,#8] */
    strh.w  r1, [r0, #8]
    bx      lr

FUNC st32_post_index        /* str r1,[r0],#4 -> returns new base */
    str     r1, [r0], #4
    bx      lr

/* ---- Unsupported form (must stop the core, never be guessed) -------- */
FUNC ldm_from_peripheral
    ldm     r0, {r1, r2}
    bx      lr
