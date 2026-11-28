.syntax unified
.thumb
.cpu cortex-m4
.section .vectors, "a"
.word 0x20400000
.word reset + 1
    .word h_nmi + 1
    .word h_hardfault + 1
    .word h_fault + 1
    .word h_fault + 1
    .word h_fault + 1
    .word 0
    .word 0
    .word 0
    .word 0
    .word h_svc + 1
    .word h_spare + 1
    .word 0
    .word h_pendsv + 1
    .word h_systick + 1
    .word h_irq0 + 1
    .word h_irq1 + 1
    .word h_irq2 + 1
    .word h_irq3 + 1
    .word h_irq4 + 1
    .word h_irq5 + 1
    .word h_irq6 + 1
    .word h_irq7 + 1
    .word h_irqx + 1
    .word h_irqx + 1
    .word h_irqx + 1
    .word h_irqx + 1
    .word h_irqx + 1
    .word h_irqx + 1
    .word h_irqx + 1
    .word h_irqx + 1
    .word h_irqx + 1
    .word h_irqx + 1
    .word h_irqx + 1
    .word h_irqx + 1
    .word h_irqx + 1
    .word h_irqx + 1
    .word h_irqx + 1
    .word h_irqx + 1
    .word h_irqx + 1
    .word h_irqx + 1
    .word h_irqx + 1
    .word h_irqx + 1
    .word h_irqx + 1
    .word h_irqx + 1
    .word h_irqx + 1
    .word h_irqx + 1
.text
.thumb_func
.global reset
reset:
.global body_start
    ldr.w r5, =0xe000ed18
    ldr.w r4, =0x00ad581e
    str.w r4, [r5]
    ldr.w r5, =0xe000ed1c
    ldr.w r4, =0x0b000000
    str.w r4, [r5]
    ldr.w r5, =0xe000ed20
    ldr.w r4, =0xc8ef0000
    str.w r4, [r5]
    ldr.w r5, =0xe000e400
    ldr.w r4, =0xe539a78b
    str.w r4, [r5]
    ldr.w r5, =0xe000e404
    ldr.w r4, =0x853d452f
    str.w r4, [r5]
    ldr.w r5, =0xe000e100
    ldr.w r4, =0x0000001f
    str.w r4, [r5]
    ldr.w r5, =0xe000ed0c
    ldr.w r4, =0x05fa0300
    str.w r4, [r5]
    ldr.w r5, =0xe000ed24
    ldr.w r4, =0x00070000
    str.w r4, [r5]
    ldr.w r5, =0xe000ed14
    ldr.w r4, =0x00000018
    str.w r4, [r5]
    ldr.n r7, 7f
    b.n 6f
    .p2align 2
7:
    .word 0x20001000
6:
    ldr.n r0, [r7, #32]
    mov.n r8, r0
    ldr.n r0, [r7, #36]
    mov.n r9, r0
    ldr.n r0, [r7, #40]
    mov.n r10, r0
    ldr.n r0, [r7, #44]
    mov.n r11, r0
    ldr.n r0, [r7, #48]
    mov.n r12, r0
    ldr.n r0, [r7, #56]
    mov.n lr, r0
    ldr.n r0, [r7, #52]
    mov.n sp, r0
    ldr.n r0, [r7, #60]
    ldr.n r1, [r7, #56]
    cmp.n r0, r1
    ldr.n r0, [r7, #0]
    ldr.n r1, [r7, #4]
    ldr.n r2, [r7, #8]
    ldr.n r3, [r7, #12]
    ldr.n r4, [r7, #16]
    ldr.n r5, [r7, #20]
    ldr.n r6, [r7, #24]
body_start:
    lsls.n r5, r7, #0
    ldmia.n r5!, {r0}
    iteee gt
    cmngt.n r2, r6
    lslle.n r0, r3
    sbcsle.w r9, r1, #0xfa00
    addle.n r3, sp, #372
    smultt r0, r7, r6
    tst.w r7, #0x500050
    ldrsh.n r0, [r7, r6]
    usat16 r8, #9, r10
    strb.n r7, [r7, #9]
    itete ls
    lsrls.n r1, r4, #18
    mulhi.n r1, r1, r1
    addls.n r4, r2, r5
    pophi.n {r3, r4, r5}
    str.n r2, [r7, r6]
    ldr.n r2, =0xd3048579
    mov.w r5, r7
    ldmia.w r5!, {r1, r3, r4, r8, r10}
    cmn.w r5, #0x47004700
    cbnz r0, 91f
    ldr.n r1, =0xb708e155
    asrs.n r4, r1, #3
91:
    cmn.w r3, #0x260026
    mov.w r5, r7
    ldrd r12, r10, [r5], #-636
    subs.w r1, r10, #0x8f008f
    uxtb16 r11, r12, ror #8
    pkhbt r5, r7, r0
    cbnz r5, 92f
    ldrh.n r1, [r7, r6]
    rsbs.n r5, r7, #0
    rev16.n r3, r7
    rsbs.n r0, r3, #0
92:
    mov.w r5, r7
    ldmia.w r5!, {r0, r1, r3, r4, r9, r10}
    ldr.w r5, =0xe000ed24
    ldr.w r0, [r5]
    muls.n r4, r5, r4
    ldr.w r5, =0xe000ed04
    ldr.w r4, =0x04000000
    str.w r4, [r5]
    smmls r1, r0, r10, r12
    b.w .Lg1
    cmp.n r0, #59
.Lg1:
    pkhbt r4, r11, r10
    mrs r5, msp
    sub.w r5, r5, #8
    msr msp, r5
    it gt
    blgt .Lg2
    ssat r11, #11, r2
.Lg2:
    add.n r4, sp, #148
    add.n sp, sp, #196
    nop.n
    mvns.w r10, #0xc700
    adr.w r8, 8f
    asrs.n r4, r6, #14
    mov.w r5, r7
    strd r1, r8, [r5, #-812]!
    cbnz r0, 93f
    cmp.n r0, #170
    rsbs.n r3, r7, #0
    muls.n r1, r4, r1
    muls.n r4, r7, r4
    ldr.n r4, =0x8507fce1
93:
    ands.n r1, r5
    mov.n r0, sp
    ldrsh.n r4, [r7, r6]
    strh.w r5, [r7, #3082]
    sdiv r5, r9, r8
    sxtab r9, r0, r8, ror #8
    cmp.n r4, #124
    sxth.n r0, r2
    msr primask, r12
    ldr.w r5, =0xe000ed20
    ldr.w r4, =0xf9068d36
    str.w r4, [r5]
    lsls.n r5, r7, #0
    ldmia.n r5!, {r2}
    b.w 94f
    .p2align 2
8:
    .word 0x410f1952
    .word 0x95230ad5
    .ltorg
94:
    and.w r5, r2, #3
    tbb [pc, r5]
.Lg3:
        .byte (.Lg4-.Lg3)/2, (.Lg5-.Lg3)/2, (.Lg6-.Lg3)/2, (.Lg7-.Lg3)/2
.p2align 1
.Lg4:
        cmp.n r4, #242
.Lg5:
        movw r1, #0x869f
.Lg6:
        subs.n r3, r2, #1
.Lg7:
        sadd8 r3, lr, r10
    pop.n {r0, r4}
    add.n r4, sp
    tst.n r2, r6
    rors.w r9, r12, #15
    pop.n {r3}
    ldr.w r4, =.Lg8+1
    push.w {r4}
    ldr.w pc, [sp], #4
    orn.w r9, r9, #0x20002000
.Lg8:
    mrs r9, epsr
    str.n r7, [sp, #44]
    cpsie f
    udf.n #200
    usax r1, r10, lr
    mvn.w r3, #0x60006000
    cbz r1, 95f
    muls.n r0, r4, r0
    add.n r5, sp, #1020
    lsls.n r2, r1, #31
95:
    itte ne
    ornne.w r9, r11, #0xce00000
    sxtahne r5, r10, r8, ror #24
    streq.w r11, [sp, #1593]
    movt r5, #0x5a36
    bhi.n 96f
    nop.n
    cmp.n r11, r5
    ldr.n r3, =0xde5909d1
96:
    nop.n
    addw r5, r7, #608
    ldrexh r8, [r5]
    clrex
    strexh r8, r2, [r5]
    push.n {r3, r4}
    sxtb.n r4, r5
    pld [r7, #2879]
    lsls.n r5, r7, #0
    ldmia.n r5!, {r1, r2, r3, r4}
    ittt hi
    addhi.n r1, sp, #692
    sbfxhi r12, r2, #17, #7
    mvnhi.w r10, #0xa0a0a0a0
    cmn.w sp, r2, lsr #28
    udiv r12, r3, r2
    cbz r5, 97f
    adds.n r3, r5, r7
    add.n r4, sp, #412
97:
    stmia.w r7, {r2, r3, r11}
    movw r4, #0x8000
    movt r4, #0x7fff
    movw r12, #0x5094
    movt r12, #0x8000
    movw r10, #0x7fff
    movt r10, #0x8000
    mrs r5, apsr
    bic.w r5, r5, #0x08000000
    msr apsr_nzcvq, r5
    smuad r2, r4, r12
    ldr.w r5, =0xe000ed28
    ldr.w r3, [r5]
    itet ge
    addge.n r5, sp
    noplt.n
    ssatge r8, #14, r7, lsl #14
    adc.w r9, lr, #0x88
    str.w r11, [r7, r6, lsl #1]
    adr.w r3, 8f
    cpsie f
    mrs r5, msp
    sub.w r5, r5, #0x800
    msr psp, r5
    mov.w r5, #2
    msr control, r5
    isb
    ldr.w r5, =0xe000e200
    ldr.w r4, =0x00000010
    str.w r4, [r5]
    ldr.w r5, =0xe000ef00
    ldr.w r4, =0x00000002
    str.w r4, [r5]
    mov.w r5, #0
    msr control, r5
    isb
    b.w 98f
    .p2align 2
8:
    .word 0xcff2fa40
    .word 0x8d379e97
    .ltorg
98:
    ldr.w r4, =.Lg9+1
    str.w r4, [r7, #124]
    add.w r5, r7, #120
    ldm.w r5, {r3, pc}
    cmp.n r11, r2
.Lg9:
    cpsie f
    svc #213
    b.n 99f
    mov.n r4, r8
    rsbs.n r2, r1, #0
    mov.n r2, r12
    ldr.n r3, =0xb12649dd
    adds.n r1, r5, r5
    add.n r1, r11
99:
    ldrb.w r0, [r7, #1326]
    cpsie f
    ite hi
    svchi #247
    addls.n r2, sp, #4
    adds.w r12, r9, r1, lsr #27
    cpsie f
    udf.n #144
    cmp.n r3, r2
    bcs.n 910f
    str.n r3, [sp, #928]
    cmp.n r2, r2
    pop.n {r1, r2}
    nop.n
910:
    udiv r8, r4, r6
    adds.n r3, r0, #1
    lsls.w r12, r2, r8
    nop.n
    msr basepri, r11
    mrs r5, msp
    sub.w r5, r5, #0x800
    msr psp, r5
    mov.w r5, #2
    msr control, r5
    isb
    push.w {r0, r1}
    pop.w {r2, r3}
    mrs r3, msp
    mov.w r5, #0
    msr control, r5
    isb
    strb.n r4, [r7, #29]
    and.w r5, r3, #3
    tbb [pc, r5]
.Lg10:
        .byte (.Lg11-.Lg10)/2, (.Lg12-.Lg10)/2, (.Lg13-.Lg10)/2, (.Lg14-.Lg10)/2
.p2align 1
.Lg11:
        sbfx r4, r11, #3, #21
.Lg12:
        bics.w r2, r2, #0xee
.Lg13:
        movt r10, #0x93ea
.Lg14:
        subw r12, lr, #232
    pkhtb r3, r8, r1, asr #11
    movs.n r5, #48
    adds.n r3, r0, r7
    muls.n r1, r3, r1
    shadd8 r12, r12, r4
    movs.n r3, #0
    subs.n r5, #124
    cmp.n r6, #223
    itett gt
    smlaldgt r0, r11, r9, lr
    strle.w r9, [sp, #3680]
    mulgt.n r3, r2, r3
    popgt.n {r1, r3}
    lsls.n r0, r0, #20
    ldrh.n r2, [r7, r6]
    mrs r5, msp
    sub.w r5, r5, #0x800
    msr psp, r5
    mov.w r5, #2
    msr control, r5
    isb
    push.w {r0, r1}
    pop.w {r2, r3}
    mov.w r5, #0
    msr control, r5
    isb
    ite gt
    revgt.n r0, r1
    mvnle.w r1, r9, lsl #14
    b.w 911f
    .p2align 2
8:
    .word 0x4dfe0106
    .word 0x04bdbaf3
    .ltorg
911:
    pkhbt r0, r3, r7, lsl #1
    ldr.w r5, =0xe000e200
    ldr.w r3, [r5]
    and.w r2, r8, #0x44004400
    ldrb.n r1, [r7, r6]
    ittt gt
    movgt.n r0, r10
    strhgt.n r6, [r7, #60]
    ldrgt.n r2, =0xf0960c75
    qsub r9, r12, r7
    cmp.n r0, #19
    push.w {r0, r2, r6, r12}
    pkhtb r11, r1, r3, asr #3
    subs.n r5, r7, #0
    cmp.n r8, r5
    itt ne
    sasxne r1, r12, r7
    blne .Lg15
    cmp.w sp, #0x7f007f
.Lg15:
    cbnz r5, 912f
    cmp.n r4, #29
    adds.n r3, r7, r1
912:
    rsbs.n r0, r5, #0
    str.n r6, [r7, r6]
    eors.w r9, lr, #0x3c
    it vs
    bvs.w .Lg16
    subs.n r1, r7, #0
.Lg16:
    uqasx r11, r11, r12
    lsls.w r2, r8, r1
    b.n 913f
    strb.n r2, [r7, #12]
913:
    rev16.w r2, r11
    lsls.n r1, r1
    uxth.n r2, r3
    rev.n r1, r7
    muls.n r1, r5, r1
    subs.n r5, r0, r3
    ldrsb.n r3, [r7, r6]
    ldr.w r4, =.Lg17+1
    str.w r4, [r7, #948]
    add.w r5, r7, #944
    ldm.w r5, {r3, pc}
    adds.n r1, r2, r7
.Lg17:
    lsls.n r0, r3
    rsbs.n r5, r6, #0
    tst.n r4, r4
    add.n r3, sp, #184
    cbz r2, 914f
    adds.n r5, r1, r2
    cmn.n r4, r1
    strb.n r2, [r7, r6]
914:
    bvc.n 915f
    lsrs.n r1, r2, #21
    adds.n r5, r6, #4
    ldrsh.n r4, [r7, r6]
915:
    cbz r4, 916f
    tst.n r4, r2
    muls.n r4, r7, r4
    add.n r0, sp, #972
    add.n sp, sp, #492
916:
    adds.n r1, r3, r2
    uxtab r11, r1, r7, ror #24
    subw r10, sp, #1528
    mul.w r12, r5, r1
    smlaltt r3, r11, r5, r3
    pop.n {r0, r1, r2}
    bgt.n 917f
    cmp.n r7, #186
    ldr.n r5, =0xef159f85
    lsls.n r1, r0, #27
    nop.n
    pop.n {r1, r3, r4}
917:
    addw r5, r7, #757
    ldrexb r11, [r5]
    movw r2, #0x964c
    eors.n r4, r2
    clrex
    strexb r8, r10, [r5]
    cpsie f
    mrs r5, msp
    sub.w r5, r5, #0x800
    msr psp, r5
    mov.w r5, #2
    msr control, r5
    isb
    svc #79
    mov.w r5, #0
    msr control, r5
    isb
    b.w 918f
    .p2align 2
8:
    .word 0x58424ad9
    .word 0x1b8a6e81
    .ltorg
918:
    cpsie f
    mrs r5, msp
    sub.w r5, r5, #0x800
    msr psp, r5
    mov.w r5, #2
    msr control, r5
    isb
    svc #191
    mov.w r5, #0
    msr control, r5
    isb
    mov.w r5, r7
    stmia.w r5!, {r1, r3}
    movs.n r4, #0x40
    msr basepri, r4
    ldr.w r5, =0xe000ef00
    ldr.w r4, =0x00000004
    str.w r4, [r5]
    mov.n r11, r9
    movs.n r4, #0
    msr basepri, r4
    mrs r11, iapsr
    bic.w r9, r8, #0x6d006d
    bfc r9, #31, #1
    mov.n r3, lr
    adr.w r3, 8b
    pop.w {r9}
    orns.w r10, r8, #0x3240
    subs.n r0, #6
    subw r4, sp, #1710
    muls.n r2, r6, r2
    mov.w r5, r7
    ldrd r3, r11, [r5], #-1008
    lsls.n r5, r7, #0
    stmia.n r5!, {r1, r2, r3, r4}
    nop.n
    cmp.w r3, #0x20
    itte pl
    strbpl.n r2, [r7, #9]
    teqpl.w r9, r3, lsr #31
    rsbmi.n r1, r0, #0
    lsls.n r5, r7, #4
    pop.n {r0, r3}
    rors.w r4, r2, #28
    cpsie f
    mov.w r5, #0x30000000
    ldrh.w r4, [r5, #2]
    subs.n r1, #168
    itet cs
    addscs.w r0, r1, r1, asr #28
    revshcc.n r2, r2
    ldrcs.n r2, =0xc74bb641
    add.n r2, sp, #4
    mrs r5, msp
    sub.w r5, r5, #0x800
    msr psp, r5
    mov.w r5, #2
    msr control, r5
    isb
    qdsub r4, r4, r10
    mov.w r5, #0
    msr control, r5
    isb
    smulwt r0, r2, r6
    it ge
    lsrge.n r5, r2, #32
    mrs r5, msp
    sub.w r5, r5, #0x800
    msr psp, r5
    mov.w r5, #2
    msr control, r5
    isb
    push.w {r0, r1}
    pop.w {r2, r3}
    mov.n r1, r11
    mov.w r5, #0
    msr control, r5
    isb
    subw sp, sp, #3572
    itte ge
    stmiage.w r7, {r2, r4, r5, r12}
    udivge r3, r5, r3
    eorslt.w r2, r4, #0xb0
    b.w 919f
    .p2align 2
8:
    .word 0x1e03cc4a
    .word 0xdc08ccf3
    .ltorg
919:
    sdiv r5, r1, r8
    adr.n r2, 8f
    cmp.n r4, r5
    and.w r5, r1, #3
    tbb [pc, r5]
.Lg18:
        .byte (.Lg19-.Lg18)/2, (.Lg20-.Lg18)/2, (.Lg21-.Lg18)/2, (.Lg22-.Lg18)/2
.p2align 1
.Lg19:
        add.n r4, sp, #764
.Lg20:
        add.w r11, r3, #0x73737373
.Lg21:
        usat r2, #12, r7, asr #4
.Lg22:
        tst.w r3, #0xbb00bb
    rbit r9, r6
    umlal r1, r10, r8, r0
    ittet ge
    uhadd16ge r0, r5, r12
    rorge.w r8, r5, r11
    umulllt r1, r0, r6, r1
    addge.n r0, sp, #340
    cmp.n r5, #134
    lsls.n r1, r2, #5
    strb.n r1, [r7, r6]
    yield.w
    nop.n
    mov.n r2, r11
    sxtb16 r0, r9, ror #8
    pop.n {r0, r2}
    cbnz r3, 920f
    cmp.n r4, r1
    cmp.n r0, #85
    lsrs.n r2, r3, #3
920:
    cbz r1, 921f
    lsls.n r4, r4, #28
    adds.n r2, #141
    asrs.n r0, r0, #30
921:
    add.n r2, pc
    adds.n r2, r6, r4
    mrs r11, iapsr
    orrs.w r12, r9, r5
    addw r1, r9, #1917
    addw r5, r7, #56
    ldrexh r10, [r5]
    strexh r2, r8, [r5]
    ldr.w r5, =0xe000ed2c
    ldr.w r0, [r5]
    sxtab16 r8, r8, r5, ror #24
    sev.w
    strbt lr, [r7, #201]
    lsls.n r5, r2, #28
    lsls.n r4, r2, #8
    pop.n {r4}
    bics.w r8, r8, #0x9b009b00
    cmp.w lr, #0x294000
    mov.n r2, pc
    nop.n
    mrs r5, control
    smlaltt r4, r10, r10, r9
    bfc r10, #3, #1
    sbcs.w r4, r0, r9
    it ne
    cmpne.w r9, r12, lsr #17
    b.n 922f
    lsls.n r5, r7, #0
    ldmia.n r5!, {r2, r4}
    lsls.n r3, r7, #7
    subs.n r1, #70
    strb.n r7, [r7, r6]
922:
    asrs.n r3, r4, #24
    add.w r11, r9, #0x7a007a00
    adds.n r0, r5, #0
    mvns.w r8, #0x1da0
    rors.n r4, r2
    sbfx r9, r6, #26, #3
    lsls.n r0, r3, #1
    uxtah r3, r11, r0, ror #24
    orns.w r8, r9, #0x51005100
    ldr.n r1, [sp, #764]
    pop.n {r0, r4}
    muls.n r4, r1, r4
    muls.n r5, r4, r5
    usat r9, #25, r9, lsl #10
    lsls.n r5, r7, #0
    ldmia.n r5!, {r3, r4}
    add.w r2, r6, #0x1a001a
    b.w 923f
    .p2align 2
8:
    .word 0xbea6221b
    .word 0x4cfeffc1
    .ltorg
923:
    clrex
    strb.n r3, [r7, #9]
    mov.w r5, r7
    ldrsb.w r1, [r5, #-133]!
    adds.n r0, r6, #6
    strd r4, r12, [r7, #-452]
    subs.n r3, r6, r5
    nop.n
    it ne
    lsrne.w r11, r5, r10
    mov.w r5, r7
    stmia.w r5!, {r1, r12}
    subs.n r0, r2, r6
    ldr.w r4, =.Lg23+1
    push.w {r3, r4}
    pop.w {r2, pc}
    muls.n r0, r0, r0
.Lg23:
    mov.w r5, r7
    ldrh.w r3, [r5, #-19]!
    sub.n sp, sp, #296
    b.n 924f
    push.n {r1, lr}
    adds.n r1, r4, r4
    pop.n {r5}
    cmp.n r5, #59
    adds.n r2, r3, r3
    muls.n r3, r2, r3
924:
    adds.n r1, r2, r5
    cmn.w r10, r8
    ldmia.w r7, {r0, r11}
    lsls.n r5, r7, #0
    stmia.n r5!, {r3}
    it hi
    movwhi r4, #0x2cc8
    push.n {r2, r4, r7}
    umlal r12, r4, r3, r2
    cpsie f
    add.w r5, r7, #2
    ldrd r0, r1, [r5]
    push.w {r5}
    ldrd r2, r8, 8f
    subw r10, r12, #1616
    ldmdb.w r7, {r3, r9, r11}
    cbnz r5, 925f
    cmp.n r12, r2
925:
    movw r9, #0x2ac7
    mov.n r8, r6
    stmdb.w r7, {r1, r10, r11}
    bpl.n 926f
    subs.n r0, #48
    add.n r9, sp
    ldrsb.n r5, [r7, r6]
926:
    qsub16 r0, r4, r2
    isb
    addw r5, r7, #414
    ldrexh r11, [r5]
    strexh r8, r10, [r5]
    adds.n r0, r2, r6
    nop.n
    str.w r4, [sp, #2756]
    smultb r1, r9, r5
    orn.w r2, r4, #0xc700c700
    sxtah r9, r3, r5, ror #8
    lsls.n r4, r1, #3
    sev.w
    movw r3, #0xb054
    mvn.w r0, #0xb300b300
    strh.w r0, [r7, #-104]
    movw r3, #0x8000
    movt r3, #0x76c6
    movw r0, #0x8466
    movt r0, #0xc000
    movw r9, #0xbe90
    movt r9, #0x800f
    mrs r5, apsr
    bic.w r5, r5, #0x08000000
    msr apsr_nzcvq, r5
    usat r10, #16, r3
    tst.w r4, r2, lsr #8
    cmp.n r4, #178
    cmp.n r10, r3
    b.w 927f
    .p2align 2
8:
    .word 0xb59a7e27
    .word 0x65237456
    .ltorg
927:
    it vs
    rsbvs.n r4, r5, #0
    rors.w r12, r9, r7
    mul.w r4, r12, lr
    eor.w r1, r8, r4, ror #19
    shsub8 r2, r6, r10
    cpsie f
    ite gt
    svcgt #160
    cmple.n r4, #239
    smmlar r11, r0, r2, r0
    lsls.w r11, r0, #20
    cpsie f
    add.w r5, r7, #3
    ldrd r0, r1, [r5]
    adr.w r10, 8b
    smmulr r5, r7, r5
    ldr.w r3, [r7, #1614]
    nop.n
    usad8 r1, r12, r9
    movs.w r2, r1
    usat r3, #28, r3, lsl #16
    it vc
    orrvc.w r8, r1, #0xe7
    str.n r4, [sp, #156]
    cbz r2, 928f
    pop.n {r5}
    ldr.n r4, [sp, #180]
    ldrh.n r4, [r7, #6]
    lsls.n r5, r7, #0
    ldmia.n r5!, {r0, r1, r2, r4}
    cmp.n r10, r6
928:
    lsls.n r3, r3, #11
    dmb
    lsls.n r3, r6, #12
    b.n 929f
    rsbs.n r1, r3, #0
    adds.n r1, r1, r0
    sbcs.n r4, r3
    nop.n
    add.n r0, sp, #352
    muls.n r0, r4, r0
929:
    bl .Lg24
    asrs.n r2, r0
    muls.n r0, r7, r0
    mov.n r12, lr
.Lg24:
    ssat r8, #19, r1, lsl #27
    movw r2, #0x207c
    lsrs.n r5, r3, #2
    adr.w r1, 8f
    ands.w r8, r10, #0x6e
    cpsie f
    mov.w r5, #0x30000000
    ldr.w r4, [r5]
    cpsid i
    ldr.w r5, =0xe000e200
    ldr.w r4, =0x00000008
    str.w r4, [r5]
    usat16 r9, #15, r10
    cpsie i
    movt r10, #0x454d
    lsls.n r5, r7, #0
    ldmia.n r5!, {r0, r1, r3}
    strd r6, r5, [r7, #220]
    pop.n {r1, r3, r5}
    ldrb.n r0, [r7, r6]
    ldmia.w r7, {r2, r5, r11}
    push.n {r1, lr}
    teq.w r4, r2, asr #25
    tst.n r1, r4
    cmp.n r9, r6
    nop.n
    b.n 930f
    rsbs.n r0, r7, #0
    asrs.n r4, r4
    lsrs.n r5, r5, #11
930:
    nop.n
    str.n r1, [sp, #552]
    cmn.n r0, r7
    bfi r5, r1, #11, #10
    ands.n r1, r1
    str.n r2, [r7, #96]
    b.w 931f
    .p2align 2
8:
    .word 0x940ba836
    .word 0xabb12761
    .ltorg
931:
    subs.w r0, sp, r11, lsl #1
    mov.w r5, r7
    ldmia.w r5!, {r0, r3, r9, r12}
    ldr.n r1, [sp, #788]
    uadd16 r5, r1, r1
    b.w .Lg25
.Lg26:
    smlaltb r9, r4, r1, r9
    b.w .Lg27
.Lg25:
    cmp.n r11, r7
    blt.w .Lg26
.Lg27:
    ldr.n r0, =0x9e0a6cad
    eor.w r8, r9, #0x10c00000
    orrs.w r4, r1, #0xe900e900
    ldr.n r4, =0xf3e12949
    nop.n
    bvc.w .Lg28
    smulwb r8, r7, r0
    add.n sp, sp, #92
    subs.n r2, r4, #2
    add.n r9, lr
.Lg28:
    cbnz r5, 932f
    subs.n r2, r3, r3
    adds.n r3, r2, r1
    adds.n r2, r0, r4
    lsls.n r5, r7, #0
    stmia.n r5!, {r0, r2, r3}
    sxtb.n r1, r6
    rsbs.n r4, r6, #0
932:
    mul.w r0, lr, r1
    sub.n sp, sp, #304
    shsub8 r4, r3, r3
    uhadd8 r1, r12, r3
    asrs.n r5, r3, #7
    and.w r5, r4, #3
    tbb [pc, r5]
.Lg29:
        .byte (.Lg30-.Lg29)/2, (.Lg31-.Lg29)/2, (.Lg32-.Lg29)/2, (.Lg33-.Lg29)/2
.p2align 1
.Lg30:
        uqsub8 r8, r7, r4
.Lg31:
        mov.w r12, #0xaa00
.Lg32:
        subs.w r11, r8, #0x3f
.Lg33:
        cmp.n r2, #82
    lsls.n r5, r7, #0
    ldmia.n r5!, {r1, r2, r3, r4}
    bne.n 933f
    movs.n r4, #162
    mov.n r8, sp
    pop.n {r0, r4, r5}
    ldrh.n r1, [r7, #50]
    cmp.n r5, #146
    ldr.n r1, [sp, #824]
933:
    pop.n {r3}
    cbz r3, 934f
    push.n {r1, r3, r7}
    adds.n r4, r2, r2
    cmp.n r8, r1
    mov.n r0, r9
934:
    ldrsh.w r1, [r7, #-22]
    mov.w r5, r7
    strd r9, r1, [r5], #-880
    movw r12, #0x1a52
    add.n r4, lr
    pop.n {r2, r4, r5}
    and.w r5, r0, #3
    tbh [pc, r5, lsl #1]
.Lg34:
        .hword (.Lg35-.Lg34)/2, (.Lg36-.Lg34)/2, (.Lg37-.Lg34)/2, (.Lg38-.Lg34)/2
.p2align 1
.Lg35:
        lsls.w r0, r8, r8
.Lg36:
        rrxs.w r8, r4
.Lg37:
        add.n r4, sp, #740
.Lg38:
        usat16 r12, #15, lr
    strh.n r5, [r7, r6]
    push.w {r1, r2, r4, r9, r10, lr}
    movs.n r4, #0x40
    msr basepri, r4
    ldr.w r5, =0xe000ef00
    ldr.w r4, =0x00000001
    str.w r4, [r5]
    rev16.n r2, r3
    movs.n r4, #0
    msr basepri, r4
    stmdb.w r7, {r9, r12}
    adc.w r2, r7, #0x20800000
    and.w r5, r2, #3
    tbh [pc, r5, lsl #1]
.Lg39:
        .hword (.Lg40-.Lg39)/2, (.Lg41-.Lg39)/2, (.Lg42-.Lg39)/2, (.Lg43-.Lg39)/2
.p2align 1
.Lg40:
        asrs.n r0, r3, #12
.Lg41:
        pkhtb r1, r7, r7, asr #12
.Lg42:
        mul.w r10, r6, r4
.Lg43:
        cmn.w r0, #0xc000c0
    b.w 935f
    .p2align 2
8:
    .word 0x5f023947
    .word 0x9f55918e
    .ltorg
935:
    cpsie f
    udf.w #56515
    muls.n r4, r3, r4
    cpsie f
    ite gt
    svcgt #165
    lsrle.n r5, r4, #21
    mov.n r2, r11
    cpsie f
    svc #175
    smmul r9, r6, r0
    cpsie f
    add.w r5, r7, #3
    strd r2, r3, [r5]
    sdiv r1, r3, r11
    adds.n r0, r6, #2
    itt cs
    addcs.n r3, r3, r4
    clrexcs
    nop.n
    uxtah r11, r4, r12, ror #16
    sel r12, r11, r6
    push.n {r2, r5, lr}
    ldr.w r5, =0xe000ef00
    ldr.w r4, =0x00000001
    str.w r4, [r5]
    cmp.n r7, r6
    cbz r5, 936f
    push.n {r0, r1}
    tst.n r0, r4
    ldrh.n r3, [r7, #34]
    adds.n r4, r1, r6
936:
    cpsie f
    add.w r5, r7, #3
    ldrex r0, [r5]
    mov.w r5, r7
    strd r1, r12, [r5, #496]!
    movs.n r4, #0x40
    msr basepri, r4
    ldr.w r5, =0xe000ef00
    ldr.w r4, =0x00000006
    str.w r4, [r5]
    subw r11, r3, #2097
    movs.n r4, #0
    msr basepri, r4
    rev16.w r8, r10
    subs.n r0, #3
    mov.w r5, r7
    ldrd r1, r11, [r5], #624
    qsub16 r0, lr, r8
    adds.n r1, r6, #0
    cpsie f
    svc #106
    itt pl
    mulpl.w r8, r3, r8
    blpl .Lg44
    add.n r1, sp, #868
.Lg44:
    cbz r5, 937f
    mov.n r10, r4
    mov.n r8, r3
937:
    ldmdb.w r7, {r4, r5}
    it ge
    cmpge.n r1, #220
    subw r10, r1, #1617
    addw r5, r7, #92
    ldrexh r8, [r5]
    strexh r8, r10, [r5]
    add.n sp, sp, #36
    b.n 938f
    mov.n r10, r11
    sxth.n r3, r0
938:
    tst.n r0, r7
    addw r5, r7, #333
    ldrexb r10, [r5]
    strexb r9, r10, [r5]
    ror.w r9, r9, r11
    ldrd r3, r10, [r7, #856]
    smlsldx r10, r1, lr, r1
    and.w r5, r4, #3
    tbh [pc, r5, lsl #1]
.Lg45:
        .hword (.Lg46-.Lg45)/2, (.Lg47-.Lg45)/2, (.Lg48-.Lg45)/2, (.Lg49-.Lg45)/2
.p2align 1
.Lg46:
        subs.n r0, r7, r1
.Lg47:
        uxtb.n r4, r7
.Lg48:
        mvn.w r0, #0xd3
.Lg49:
        adds.w r1, r2, #0x9e009e00
    b.w 939f
    .p2align 2
8:
    .word 0xc71f26f0
    .word 0xef09b47b
    .ltorg
939:
    ldr.n r1, =0xcb3626b5
    ldrsh.n r1, [r7, r6]
    itt hi
    movhi.w r0, r5
    blhi .Lg50
    subw r1, sp, #1081
.Lg50:
    lsls.n r5, r7, #0
    ldmia.n r5!, {r0, r1, r3, r4}
    uqsax r8, r2, r10
    rsbs.n r5, r2, #0
    adds.n r3, r4, #0
    push.n {r0, r7}
    itt mi
    ldrmi.n r2, =0xc593d4b9
    popmi.n {r0, r2, r5}
    mvns.w r5, r3, asr #20
    cbnz r2, 940f
    mov.n r11, r8
    pop.n {r3, r4, r5}
    sxth.n r5, r7
    cmp.n r2, r4
940:
    uhadd8 r5, r3, r0
    cmp.n r9, r4
    ite hi
    ldrthi r8, [r7, #96]
    asrls.n r5, r2, #12
    bcc.n 941f
    uxth.n r5, r5
    lsls.n r5, r1, #2
941:
    cbnz r2, 942f
    adds.n r4, r5, r0
    str.n r2, [sp, #120]
    subs.n r3, #79
    movs.n r1, #133
942:
    muls.n r0, r6, r0
    mov.n r4, r8
    ldr.n r4, =0xa2f4233d
    pop.n {r0, r3, r5}
    ldrh.n r1, [r7, #32]
    mov.w r9, r2
    mrs r10, iepsr
    lsls.n r2, r0, #5
    cbz r1, 943f
    rors.n r3, r5
    subs.n r5, r7, r2
943:
    nop.n
    bic.w r11, r2, #0x78007800
    ubfx r0, lr, #13, #17
    orn.w r8, r12, r8
    nop.n
    cpsie f
    udf.n #221
    orrs.w r11, lr, #0x1c001c00
    tst.n r4, r6
    ldr.w r5, =0xe000ef00
    ldr.w r4, =0x00000004
    str.w r4, [r5]
    itt ge
    mulge.w r0, r10, r12
    bge.w .Lg51
    sbcs.n r1, r3
.Lg51:
    usada8 r2, r7, r3, r3
    mov.n r5, r9
    mvn.w r8, #0x37003700
    add.n r2, sp, #180
    mov.w r5, r7
    stmdb.w r5!, {r0, r3, r8, r9, r10, r12, lr}
    rsbs.n r5, r4, #0
    cmp.n r2, r4
    smlad r11, r10, r8, r7
    adds.n r4, r2, r6
    cpsie f
    mov.w r5, #0
    udiv r4, r0, r5
    strd r5, r5, [r7, #-348]
    ldr.n r1, [r7, #84]
    itet gt
    msrgt basepri_max, r9
    orrle.w r3, r6, #0x500000
    subgt.n sp, sp, #268
    cpsie f
    ldr.w r5, =0xE000ED04
    mov.w r4, #0x10000000
    itt mi
    strmi.w r4, [r5]
    adrmi.w r0, 8f
    b.w 944f
    .p2align 2
8:
    .word 0x9cc7748d
    .word 0xea4116b5
    .ltorg
944:
    ldrb.n r1, [r7, r6]
    add.n r1, r8
    lsrs.w r10, r12, #22
    cmp.n r5, #223
    ldr.n r2, [sp, #736]
    uxtab r1, r6, lr, ror #8
    muls.n r0, r4, r0
    bvc.n 945f
    cmp.n r5, #11
    movs.n r3, #142
    subs.n r4, r2, #4
945:
    strd r1, lr, [r7, #-260]
    ldr.w r1, [sp, #855]
    and.w r5, r0, #3
    tbb [pc, r5]
.Lg52:
        .byte (.Lg53-.Lg52)/2, (.Lg54-.Lg52)/2, (.Lg55-.Lg52)/2, (.Lg56-.Lg52)/2
.p2align 1
.Lg53:
        movs.w r8, #0xc500c500
.Lg54:
        nop.n
.Lg55:
        sxtab16 r1, r1, r2, ror #8
.Lg56:
        subs.n r4, r0, #4
    subs.w r10, r8, #0x74000
    itee gt
    revshgt.n r4, r5
    addle.n r4, r1, r7
    addle.n r3, r2, #2
    ldrsh.w r2, [r7, #3134]
    ite vc
    andsvc.w r3, r0, #0x950095
    addvs.n r5, sp, #436
    lsls.n r5, r7, #0
    stmia.n r5!, {r0, r1, r4}
    movw r2, #0x8000
    movt r2, #0x7fff
    movw r8, #0x4221
    movt r8, #0x62a5
    movw r0, #0x847d
    movt r0, #0x800f
    mrs r5, apsr
    bic.w r5, r5, #0x08000000
    msr apsr_nzcvq, r5
    usat r9, #5, r2
    cmp.n r11, r1
    ldr.w r2, [r7, #4064]
    bhi.w .Lg57
    add.n r2, sp, #144
    rsbs.n r1, r3, #0
.Lg57:
    str.n r2, [sp, #948]
    lsls.n r4, r1, #19
    bl .Lg58
    shsax r1, r8, r12
    add.n r4, sp, #724
.Lg58:
    ldr.w r5, =0xe000ed20
    ldr.w r4, =0x764020d3
    str.w r4, [r5]
    ldr.n r2, [sp, #832]
    subs.n r4, #101
    muls.n r2, r0, r2
    adds.n r0, r6, r4
    cbnz r4, 946f
    add.n r2, sp, #920
946:
    eors.n r4, r1
    movw r12, #0x7fff
    movt r12, #0x8000
    movw r8, #0x7fff
    movt r8, #0x7fff
    movw r10, #0x8000
    movt r10, #0x7ff0
    mrs r5, apsr
    bic.w r5, r5, #0x08000000
    msr apsr_nzcvq, r5
    smlsdx r0, r12, r8, r10
    movw r4, #0x2a99
    movt r4, #0x8000
    movw r2, #0x7fff
    movt r2, #0x8000
    movw r10, #0x8000
    movt r10, #0x7ff0
    mrs r5, apsr
    bic.w r5, r5, #0x08000000
    msr apsr_nzcvq, r5
    ssat16 r8, #12, r4
    b.w 947f
    .p2align 2
8:
    .word 0xd5e278cd
    .word 0xd16fcd54
    .ltorg
947:
    it mi
    ssatmi r4, #9, r7
    and.w r5, r4, #3
    tbb [pc, r5]
.Lg59:
        .byte (.Lg60-.Lg59)/2, (.Lg61-.Lg59)/2, (.Lg62-.Lg59)/2, (.Lg63-.Lg59)/2
.p2align 1
.Lg60:
        sadd16 r0, r12, r6
.Lg61:
        rsbs.n r2, r6, #0
.Lg62:
        pkhbt r8, r7, r9
.Lg63:
        cmp.n r4, #26
    ldrb.n r4, [r7, #27]
    addw r5, r7, #134
    ldrexh r2, [r5]
    strexh r2, r8, [r5]
    mrs r5, msp
    sub.w r5, r5, #0x800
    msr psp, r5
    mov.w r5, #2
    msr control, r5
    isb
    smulbb r8, lr, r6
    mrs r11, msp
    movw r1, #0xcc5e
    mov.w r5, #0
    msr control, r5
    isb
    cmp.w r3, #0x39c0000
    itt gt
    ldrshgt.n r5, [r7, r6]
    addgt.n r11, r5
    itt cc
    sbccc.n r0, r6
    movcc.n r0, lr
    nop.n
    itete gt
    lslgt.n r5, r1, #11
    smlabble r2, r4, r3, lr
    addgt.n r3, r0, r2
    teqle.w r9, r2, lsr #25
    qsub16 r5, r10, r9
    mrs r5, eapsr
    cbnz r0, 948f
    strh.n r1, [r7, #2]
    push.n {r1, lr}
948:
    and.w r3, r9, r1, rrx
    orrs.w r3, r10, #0x26002600
    sxtb.n r4, r7
    lsls.n r5, r7, #0
    ldmia.n r5!, {r1, r3}
    strh.n r3, [r7, r6]
    adr.n r1, 8f
    cpsie f
    sub.w sp, sp, #4
    svc #116
    add.w sp, sp, #4
    cbnz r1, 949f
    push.n {r5}
    nop.n
    adds.n r5, r6, r6
    movs.n r0, #96
949:
    mov.w r5, r7
    strd r2, r4, [r5, #-324]!
    itt vs
    strdvs r1, r12, [r7, #288]
    popvs.n {r0, r2}
    add.n sp, sp, #412
    mrs r5, msp
    sub.w r5, r5, #128
    msr msp, r5
    add.n sp, sp, #332
    ldr.n r2, =0x9a15f091
    add.n r10, r6
    cpsie f
    ldr.w r5, =0xE000ED04
    mov.w r4, #0x10000000
    itt vs
    strvs.w r4, [r5]
    rsbvs.w r0, r6, #0x4a004a00
    sdiv r9, r11, r11
    smultb r2, r11, r8
    adr.n r5, 8f
    subs.n r2, r1, #6
    b.w 950f
    .p2align 2
8:
    .word 0xfaabec89
    .word 0xe0b75da5
    .ltorg
950:
    orn.w r5, lr, #0x7f007f00
    ittee eq
    ldrsheq.n r2, [r7, r6]
    nopeq.n
    mvnne.n r0, r6
    smusdne r0, r10, r4
    cmp.n r9, r3
    pkhbt r4, r9, r2
    adds.n r4, #161
    adds.n r3, r5, r2
    ldr.n r5, [sp, #352]
    sub.n sp, sp, #88
    pkhbt r5, r0, r0
    add.n r2, r11
    clrex
    ands.n r1, r4
    it ge
    msrge apsr_g, r0
    cpsie f
    ite ne
    svcne #123
    bfieq r1, r4, #24, #6
    lsls.n r5, r7, #0
    ldmia.n r5!, {r2}
    push.n {r0, r7}
    addw r5, r7, #836
    ldrexh r9, [r5]
    strexh r9, r11, [r5]
    cbz r4, 951f
    adds.n r0, r4, r6
    pop.n {r0, r2, r4}
    add.n r2, sp, #1012
951:
    movw r0, #0x8000
    movt r0, #0x8000
    movw r12, #0x7fff
    movt r12, #0x8000
    movw r2, #0x7fff
    movt r2, #0x7fff
    mrs r5, apsr
    bic.w r5, r5, #0x08000000
    msr apsr_nzcvq, r5
    ssat r10, #24, r0
    cpsie f
    mov.w r5, #0
    udiv r4, r0, r5
    cmp.w r8, r11, ror #24
    and.w r5, r4, #3
    tbh [pc, r5, lsl #1]
.Lg64:
        .hword (.Lg65-.Lg64)/2, (.Lg66-.Lg64)/2, (.Lg67-.Lg64)/2, (.Lg68-.Lg64)/2
.p2align 1
.Lg65:
        adds.n r3, r6, r1
.Lg66:
        mov.n r1, r8
.Lg67:
        orns.w r11, r10, r0, asr #16
.Lg68:
        add.n r8, r4
    adr.n r4, 8f
    ittt lt
    andslt.w r3, r1, #0x7f7f7f7f
    ldrhlt.n r0, [r7, #38]
    sxthlt.w r9, r10, ror #24
    mvns.w r2, #0x24002400
    pkhtb r12, r9, r6, asr #29
    pop.n {r0, r2, r5}
    lsls.n r0, r5
    rsbs.n r4, r6, #0
    it ls
    usatls r10, #20, r1
    ite ls
    sxthls.n r0, r7
    movshi.w r0, #0x5c5c5c5c
    lsrs.n r2, r0, #32
    ldr.n r1, [r7, #112]
    clrex
    ldr.n r1, =0xa5ba0f71
    lsls.n r5, r0, #4
    itet ne
    subwne r3, r0, #1698
    adreq.w r12, 8b
    subne.n r2, r5, r1
    str.n r2, [sp, #776]
    nop.n
    cbnz r5, 952f
    add.n sp, sp, #300
    subs.n r2, #2
    adds.n r1, r2, r5
952:
    b.w 953f
    .p2align 2
8:
    .word 0xdece2851
    .word 0xa001cf92
    .ltorg
953:
    cpsie f
    mrs r5, msp
    sub.w r5, r5, #0x800
    msr psp, r5
    mov.w r5, #2
    msr control, r5
    isb
    svc #24
    mov.w r5, #0
    msr control, r5
    isb
    cmp.n r1, #126
    lsls.n r5, r7, #0
    stmia.n r5!, {r0, r2, r3, r4}
    ldrb.n r3, [r7, #2]
    pop.n {r0, r4, r5}
    rors.w r4, r9, r8
    rsbs.n r0, r2, #0
    mov.n r0, r12
    cmp.w r6, r8, rrx
    ittee ne
    nopne.n
    selne r10, r1, r7
    addeq.n r0, sp, #800
    uhadd16eq r4, r5, r12
    pop.n {r1, r4, r5}
    sbcs.n r3, r3
    ands.n r4, r2
    pkhtb r0, lr, r10, asr #31
    uxtab16 r1, r1, lr, ror #24
    rev.w r4, r3
    pld [r7, r6, lsl #3]
    push.w {r1, r3, r7, r8, r12}
    add.n r5, sp, #456
    add.n r5, sp, #972
    cmp.n r11, r6
    bl .Lg69
    teq.w r2, r9
    muls.n r4, r7, r4
    usad8 r10, r8, lr
    cmp.w sp, r11, asr #13
.Lg69:
    stmia.w r7, {r8, r9, r10, r12}
    bmi.w .Lg70
    add.n sp, sp, #192
    movt r10, #0xcab9
    mvn.w r12, #0xf8
    smultt r1, r11, r2
.Lg70:
    itee cs
    mvncs.w r9, #0x70800
    rsbscc.w r8, r11, r10, lsl #26
    lslcc.n r3, r4
    adcs.w r8, r8, #0xc900c9
    cpsie f
    ldr.w r5, =0xE000ED04
    mov.w r4, #0x10000000
    itt gt
    strgt.w r4, [r5]
    mlsgt r4, r1, r10, r10
    mrs r5, msp
    sub.w r5, r5, #72
    msr msp, r5
    msr apsr_nzcvqg, r0
    uhadd16 r5, r7, r3
    sub.n sp, sp, #452
    adds.n r1, r0, r7
    adr.w r3, 8f
    cmn.w sp, r3, lsl #7
    pop.n {r1, r5}
    sbc.w r11, r4, #0xa6
    ldr.w r4, =.Lg71+1
    str.w r4, [r7, #692]
    add.w r5, r7, #688
    ldm.w r5, {r3, pc}
    isb
.Lg71:
    ldr.w r5, =0xe000e200
    ldr.w r4, =0x00000002
    str.w r4, [r5]
    adr.w r1, 8b
    add.w r9, r7, #0x770077
    adr.n r3, 8f
    smmlar r4, r12, r1, r0
    b.w 954f
    .p2align 2
8:
    .word 0x3647de3d
    .word 0x7e3fd870
    .ltorg
954:
    cpsie f
    sub.w sp, sp, #4
    ldr.w r5, =0xe000ef00
    ldr.w r4, =0x00000000
    str.w r4, [r5]
    add.w sp, sp, #4
    cmp.n r12, r5
    ldr.w r5, =0xe000ef00
    ldr.w r4, =0x00000005
    str.w r4, [r5]
    smull r11, r5, r1, r2
    sdiv r5, lr, r8
    tst.n r0, r7
    ldr.w r5, =0xe000e280
    ldr.w r4, =0x00000040
    str.w r4, [r5]
    mov.w r5, r7
    strd r11, r9, [r5], #688
    blt.n 955f
    pop.n {r1, r2, r5}
    pop.n {r4}
    ldr.n r5, =0x92a82049
955:
    ldrsb.w r5, [r7, r6, lsl #3]
    mrs r5, msp
    sub.w r5, r5, #252
    msr msp, r5
    stmdb.w r7, {r1, r2, r4, r5, r12}
    cbnz r5, 956f
    rev16.n r1, r3
    rsbs.n r0, r7, #0
    add.n r8, r6
    str.n r2, [r7, #24]
    muls.n r1, r2, r1
956:
    mov.w r5, r7
    stmia.w r5!, {r2, r11}
    ldr.w r4, =.Lg72+1
    push.w {r4}
    ldr.w pc, [sp], #4
    lsr.w r9, r0, r4
.Lg72:
    ittt cc
    usat16cc r10, #6, r4
    qsubcc r4, r5, r10
    popcc.w {r12}
    rsbs.n r4, r0, #0
    mvns.w r8, r3, lsr #28
    lsrs.n r4, r6, #7
    sub.n sp, sp, #20
    itt cs
    ldrshcs.w r10, [r7, #-38]
    mvncs.w r8, #0x50
    sxtab r4, r11, lr, ror #24
    iteet lt
    sxtablt r4, r6, r3
    lslge.n r3, r5, #29
    rsbge.n r1, r6, #0
    uxtblt.w r10, r9, ror #24
    mov.w r5, r7
    stmdb.w r5!, {r3, r8, r10, r12}
    addw r8, r6, #3814
    it pl
    bpl.w .Lg73
    cmp.w r9, r11
.Lg73:
    and.w r5, r1, #3
    tbh [pc, r5, lsl #1]
.Lg74:
        .hword (.Lg75-.Lg74)/2, (.Lg76-.Lg74)/2, (.Lg77-.Lg74)/2, (.Lg78-.Lg74)/2
.p2align 1
.Lg75:
        uxtb16 r2, r2, ror #24
.Lg76:
        nop.n
.Lg77:
        qadd r3, r3, r9
.Lg78:
        nop.n
    muls.n r4, r3, r4
    mrs r5, msp
    sub.w r5, r5, #244
    msr msp, r5
    mrs r5, msp
    sub.w r5, r5, #8
    msr msp, r5
    ittt vc
    mvnsvc.w r8, #0x170017
    dsbvc
    sbcsvc.w r9, r10, #0x480048
    b.w 957f
    .p2align 2
8:
    .word 0x3ef59c37
    .word 0x97d269c9
    .ltorg
957:
    ldr.n r0, [r7, #100]
    addw r12, lr, #2378
    pop.n {r2, r3, r4}
    itttt lt
    stmialt.w r7, {r2, r10, r11, r12}
    pushlt.w {r1}
    strlt.w r6, [sp, #3872]
    addlt.n r4, r7, r0
    adr.w r3, 8b
    rsbs.n r4, r0, #0
    sxtb.w r5, r10, ror #24
    sub.w r1, sp, r2, lsl #3
    udiv r11, r1, r11
    tst.n r2, r6
    mov.w r5, r7
    ldrd r12, r11, [r5, #24]!
    cpsie f
    ldr.w r5, =0xE000ED04
    mov.w r4, #0x10000000
    itt lt
    strlt.w r4, [r5]
    sbcslt.w r12, r2, #0x90909090
    cpsid i
    ldr.w r5, =0xe000e200
    ldr.w r4, =0x00000002
    str.w r4, [r5]
    and.w r0, r10, #0x2d2d2d2d
    cpsie i
    cbnz r3, 958f
    subs.n r4, #249
    adds.n r3, r0, r3
958:
    cbz r0, 959f
    subs.n r1, #241
    cmp.n r5, #34
    lsls.n r1, r1, #5
    cmp.n r5, #149
    nop.n
    nop.n
959:
    strht r8, [r7, #181]
    adds.n r0, r6, #2
    mul.w r10, r12, r1
    cpsid i
    ldr.w r5, =0xe000e200
    ldr.w r4, =0x00000001
    str.w r4, [r5]
    nop.n
    cpsie i
    cmp.w r4, lr, ror #13
    sub.n sp, sp, #144
    lsls.n r5, r7, #0
    ldmia.n r5!, {r1, r4}
    movs.n r4, #119
    rsbs.n r5, r1, #0
    str.w r6, [sp, #2673]
    cpsie f
    mov.w r5, #0
    sdiv r4, r0, r5
    push.n {r6}
    rsbs.n r1, r4, #0
    movs.w r8, #0x38383838
    qdsub r10, r5, r4
    ittet gt
    addgt.n r4, r4, #2
    addwgt r5, r1, #2676
    sbcsle.w r11, r9, #0x4a004a
    sxtb16gt r10, r4, ror #8
    udiv r10, r2, r0
    mov.w r5, r7
    ldrd r8, r12, [r5, #400]!
    mov.w r5, r7
    ldmdb.w r5!, {r0, r4, r8, r9, r11}
    mov.n r4, pc
    adds.n r0, r4, r4
    movs.n r4, #0x40
    msr basepri, r4
    ldr.w r5, =0xe000ef00
    ldr.w r4, =0x00000005
    str.w r4, [r5]
    mls r12, r7, r6, r4
    movs.n r4, #0
    msr basepri, r4
    b.w 960f
    .p2align 2
8:
    .word 0xa8f45e45
    .word 0x2903584a
    .ltorg
960:
    addw sp, sp, #1340
    msr basepri_max, r11
    mov.w r5, r7
    stmdb.w r5!, {r2, r3, r8, r9, r10, r12}
    adds.w r10, r5, #0xf0
    muls.n r2, r1, r2
    pop.n {r1}
    strb.w r5, [r7, #934]
    addw r5, r7, #894
    ldrexh r11, [r5]
    mvns.w r1, r12, lsl #1
    strexh r0, r2, [r5]
    nop.n
    movt r1, #0xa3fb
    mla r5, r6, r2, r8
    and.w r5, r1, #3
    tbh [pc, r5, lsl #1]
.Lg79:
        .hword (.Lg80-.Lg79)/2, (.Lg81-.Lg79)/2, (.Lg82-.Lg79)/2, (.Lg83-.Lg79)/2
.p2align 1
.Lg80:
        smlaldx r2, r4, r9, r10
.Lg81:
        muls.n r2, r0, r2
.Lg82:
        lsl.w r12, r10, #28
.Lg83:
        subs.n r1, r6, #5
    nop.n
    cpsie f
    svc #68
    ldr.n r5, =0xb38ce971
    b.w .Lg84
.Lg85:
    add.n r4, r9
    b.w .Lg86
.Lg84:
    subs.n r3, r1, #1
    bl .Lg85
.Lg86:
    muls.n r1, r0, r1
    push.w {r7, r9, r11, lr}
    ldr.w r4, =.Lg87+1
    push.w {r3, r4}
    pop.w {r2, pc}
    nop.n
.Lg87:
    strb.w r3, [r7, r6, lsl #3]
    stmdb.w r7, {r0, r2, r4, r11}
    cmp.w sp, #0x1a000
    revsh.w r5, r2
    msr basepri, r9
    mul.w r0, r10, r5
    uxtb16 r10, r11
    ite mi
    ssatmi r10, #19, r7, asr #8
    qsub16pl r2, r10, lr
    push.n {r0, r2, r5, r6}
    stmia.w r7, {r2, r3, r4, r8, r12}
    subs.n r2, r0, r0
    bpl.n 961f
    mov.n r5, r8
    subs.n r0, #39
961:
    nop.n
    adr.w r2, 8b
    ror.w r1, r4, r11
    cpsie f
    svc #224
    ite gt
    smlatbgt r8, r0, r11, r5
    asrle.n r1, r0, #22
    cpsie f
    ite pl
    svcpl #210
    addmi.n r2, r4, #1
    bfi r4, lr, #14, #9
    lsls.n r0, r0, #11
    cmn.w sp, r6, ror #9
    movw r0, #0x7fff
    movt r0, #0x4000
    movw r4, #0x8000
    movt r4, #0x7fff
    movw r10, #0xf8e7
    movt r10, #0x800f
    mrs r5, apsr
    bic.w r5, r5, #0x08000000
    msr apsr_nzcvq, r5
    qadd r8, r0, r4
    it cs
    blcs .Lg88
    shsax r3, r10, r11
.Lg88:
    b.w 962f
    .p2align 2
8:
    .word 0x5b0dd016
    .word 0xd839138e
    .ltorg
962:
    cmp.w r5, #0x8d008d
    cmn.n r7, r6
    mov.w r5, r7
    ldmdb.w r5!, {r2, r11}
    ldrbt r11, [r7, #176]
    push.n {r1, r3, r5, lr}
    cbz r3, 963f
    subs.n r2, r6, r7
    ldr.n r3, [sp, #344]
    subs.n r2, r0, r2
    lsls.n r1, r3, #21
963:
    push.n {r0, r7}
    cpsie f
    ldr.w r5, =0xE000ED04
    mov.w r4, #0x10000000
    itt lt
    strlt.w r4, [r5]
    cmnlt.w r4, r7, asr #2
    cpsie f
    ite eq
    svceq #38
    smultbne r3, r1, r8
    adds.n r5, #237
    mrs r11, apsr
    bls.n 964f
    str.n r2, [sp, #664]
    muls.n r3, r6, r3
    adds.n r3, r6, r4
    cmp.n r9, r4
    muls.n r4, r0, r4
964:
    mov.w r5, r7
    ldrd r4, r3, [r5, #-664]!
    uxtb.n r0, r3
    mls r11, r1, r3, r1
    lsls.n r5, r7, #0
    ldmia.n r5!, {r0, r1, r4}
    adr.w r5, 8f
    muls.n r1, r7, r1
    cmp.n r12, r6
    movw r11, #0x7fff
    movt r11, #0x7fff
    movw r8, #0x8000
    movt r8, #0x8000
    movw r1, #0x8000
    movt r1, #0x800f
    mrs r5, apsr
    bic.w r5, r5, #0x08000000
    msr apsr_nzcvq, r5
    smlatb r9, r11, r8, r1
    revsh.n r3, r0
    mrs r5, msp
    sub.w r5, r5, #176
    msr msp, r5
    sxtb.w r11, r3, ror #24
    adcs.w r9, r2, #0x1d4000
    subs.n r4, #173
    rsbs.n r5, r4, #0
    subs.n r4, r5, r2
    lsls.n r5, r7, #0
    ldmia.n r5!, {r2}
    itee gt
    rsbgt.n r0, r2, #0
    movwle r4, #0x562a
    ldrdle r0, r2, [r7, #560]
    orrs.w r2, lr, #0x35003500
    itet hi
    usada8hi r11, r10, r0, r5
    rsbls.w r0, r10, #0x140014
    rsbhi.n r3, r5, #0
    b.w .Lg89
.Lg90:
    qadd8 r0, r9, lr
    b.w .Lg91
.Lg89:
    ands.n r4, r2
    bl .Lg90
.Lg91:
    ldr.w r5, =0xe000ed20
    ldr.w r4, =0x61f6808b
    str.w r4, [r5]
    add.n r5, pc
    subs.w r10, r5, #0x7d7d7d7d
    cpsie f
    ite hi
    svchi #127
    subls.n sp, sp, #0
    b.w 965f
    .p2align 2
8:
    .word 0xba237863
    .word 0x2536d0de
    .ltorg
965:
    subs.n r0, r0, r0
    rrx.w r10, r3
    mov.w r5, r7
    ldrsh.w r3, [r5], #-182
    lsls.n r3, r1, #10
    b.n 966f
    subs.n r4, r1, r7
    adcs.n r2, r4
    add.n r1, sp, #192
    tst.n r5, r1
    nop.n
    cmn.n r1, r4
966:
    qasx r9, r10, r10
    adds.n r4, r1, r4
    cpsie f
    mov.w r5, #0
    sdiv r4, r0, r5
    adr.n r5, 8f
    str.w r5, [r7, #2142]
    cmp.n r6, #127
    addw r5, r7, #596
    ldrexh r3, [r5]
    nop.n
    eors.w r11, r6, #0x36003600
    strexh r2, r12, [r5]
    sdiv r12, r11, r0
    addw r3, r10, #3389
    lsls.n r3, r4, #13
    cpsie f
    svc #51
    muls.n r3, r5, r3
    add.n sp, sp, #64
    itt mi
    uqsub16mi r1, r6, r9
    blmi .Lg92
    lsl.w r3, r9, r6
.Lg92:
    smlawt r3, r3, r10, r9
    ldr.w r5, =0xe000e280
    ldr.w r4, =0x00000001
    str.w r4, [r5]
    cmn.w r4, r8, rrx
    adds.n r4, r5, #5
    pop.w {r0}
    smlsdx r8, r0, r4, r10
    movs.n r4, #209
    ldr.w r5, =0xe000ed08
    ldr.w r4, [r5]
    nop.n
    mov.n r10, r1
    rev16.n r2, r3
    str.n r0, [sp, #416]
    bl .Lg93
    asrs.w r0, r9, r6
    asrs.n r0, r7, #10
    rsbs.n r0, r4, #0
.Lg93:
    subs.n r4, r5, #7
    subs.n r0, r1, #2
    cmp.n r3, #238
    strd r5, r4, [r7, #980]
    ands.w r2, r11, lr, lsr #6
    usad8 r2, r1, r10
    cpsie f
    mov.w r5, #0
    udiv r4, r0, r5
    sub.n sp, sp, #352
    bl .Lg94
    sxtb16 r3, r7
    cmn.n r2, r6
.Lg94:
    ldr.w r4, =.Lg95+1
    str.w r4, [r7, #504]
    ldr.w pc, [r7, #504]
    adc.w r11, r2, #0x63006300
.Lg95:
    adds.n r4, r1, r5
    udiv r1, r11, r7
    it vs
    usada8vs r4, r6, r4, lr
    push.w {r0, r3, r5, r11}
    ite ge
    smulbtge r12, r10, r6
    addlt.n r3, r6, r2
    b.w 967f
    .p2align 2
8:
    .word 0x67723e2b
    .word 0x52d0664c
    .ltorg
967:
    nop.n
    adds.n r3, r1, #0
    ldr.w r11, [sp, #3206]
    cmp.n r4, #10
    usax r8, r4, r8
    movt r10, #0x7cf
    ittee vs
    addvs.n r3, r6, #2
    strdvs r12, r9, [r7, #-820]
    sxtbvc.w r10, r9, ror #8
    smlsdvc r11, lr, r12, r11
    qasx r8, r7, r3
    sxtb16 r3, r6
    qsub r1, r8, r7
    smulwt r4, lr, r10
    subs.n r3, r4, r1
    cpsie f
    udf.n #82
    b.n 968f
    nop.n
    adds.n r2, r2, #5
968:
    itte vs
    addvs.n r12, r6
    clrexvs
    msrvc apsr_nzcvqg, r7
    qasx r5, r6, r0
    uxtb.n r0, r3
    ble.n 969f
    revsh.n r2, r3
969:
    smlatt r12, r12, r3, r6
    lsls.n r3, r3, #27
    rsbs.n r0, r3, #0
    ldr.n r3, =0xa5e03a11
    adr.n r2, 8f
    ldr.w r4, =.Lg96+1
    str.w r4, [r7, #164]
    add.w r5, r7, #160
    ldm.w r5, {r3, pc}
    movs.n r2, #29
.Lg96:
    muls.n r1, r4, r1
    cmp.n r7, #112
    ittt gt
    subgt.n sp, sp, #224
    movtgt r4, #0x656a
    nopgt.n
    stmia.w r7, {r1, r5, r9, r11}
    itet le
    eorle.w r2, r6, #0x4c4c4c4c
    cmpgt.n r3, #236
    strdle r2, r3, [r7, #-852]
    usat16 r4, #3, r10
    mov.w r5, r7
    ldr.w r2, [r5, #138]!
    ldrh.n r4, [r7, #22]
    cmp.n r9, r1
    pkhbt r9, r1, r9, lsl #26
    mvns.n r1, r6
    nop.n
    it ge
    qasxge r11, r1, r4
    bics.n r0, r3
    umaal r4, r3, lr, r3
    push.n {r1, r4, r6}
    push.n {r2, r4, lr}
    mla r8, r1, r5, r2
    adr.w r9, 8f
    cmn.w r8, r8, lsl #13
    cpsie f
    udf.n #10
    itete le
    mulle.n r5, r5, r5
    usat16gt r1, #12, r11
    nople.n
    cmpgt.w r4, #0x32323232
    adds.n r3, r7, r2
    mrs r5, msp
    sub.w r5, r5, #0x800
    msr psp, r5
    mov.w r5, #2
    msr control, r5
    isb
    uadd16 r9, r6, lr
    push.w {r0, r1}
    pop.w {r2, r3}
    push.w {r0, r1}
    pop.w {r2, r3}
    mov.w r5, #0
    msr control, r5
    isb
    b.w 970f
    .p2align 2
8:
    .word 0x253cd422
    .word 0xa38015cc
    .ltorg
970:
    cmp.n r2, #58
    bvc.n 971f
    rsbs.n r5, r7, #0
    cmp.n r5, #75
    lsrs.n r2, r0
971:
    and.w r5, r4, #3
    tbb [pc, r5]
.Lg97:
        .byte (.Lg98-.Lg97)/2, (.Lg99-.Lg97)/2, (.Lg100-.Lg97)/2, (.Lg101-.Lg97)/2
.p2align 1
.Lg98:
        sxtab16 r1, r0, r0, ror #24
.Lg99:
        addw r10, r1, #3895
.Lg100:
        subs.n r2, r6, r1
.Lg101:
        mov.w r1, #0xc200c2
    nop.n
    mov.w r0, r5
    nop.n
    adds.n r2, r0, r5
    pop.w {r8, r11}
    push.w {r0, r7, r9, r11, r12}
    asrs.n r3, r1, #2
    add.n r2, sp, #412
    mov.w r5, r7
    ldrb.w r9, [r5], #249
    ldr.n r1, [sp, #1004]
    rsbs.n r3, r1, #0
    ble.n 972f
    lsrs.n r2, r2, #5
972:
    ldrd r3, r2, 8b
    add.n r8, r1
    lsls.n r4, r3, #9
    strb.n r7, [r7, #5]
    cbz r1, 973f
    mvns.n r5, r3
973:
    itte vc
    cmpvc.n r12, r2
    sxtbvc.n r3, r6
    smmlsvs r10, r8, r6, r8
    ldr.w r4, =.Lg102+1
    str.w r4, [r7, #936]
    add.w r5, r7, #932
    ldm.w r5, {r3, pc}
    add.n r4, sp, #820
.Lg102:
    adcs.w r5, r5, r12
    ldr.n r5, =0xdcfaf80d
    cpsie f
    add.w r5, r7, #2
    ldm.w r5, {r0, r1}
    mov.w r5, r7
    ldrd r0, r10, [r5, #260]!
    lsls.n r5, r7, #0
    stmia.n r5!, {r0, r1, r3}
    bfc r2, #13, #2
    cbnz r2, 974f
    lsls.n r5, r7, #0
    stmia.n r5!, {r0, r1, r3}
    mov.n r10, r12
974:
    ldrsh.n r2, [r7, r6]
    rev.w r8, r4
    pli [r7, #5]
    pld [r7, #-65]
    nop.n
    cmp.n r9, r4
    ldr.w r5, =0xe000ed20
    ldr.w r3, [r5]
    pop.n {r2, r5}
    adds.n r5, r4, r0
    it le
    addle.n r3, r6, r1
    sub.w r9, sp, r10, lsr #5
    mov.w r5, r7
    str.w r12, [r5, #44]!
    mrs r5, msp
    sub.w r5, r5, #0x800
    msr psp, r5
    mov.w r5, #2
    msr control, r5
    isb
    usat r8, #2, r6
    mrs r9, msp
    movs.w r12, #0xe8
    mov.w r5, #0
    msr control, r5
    isb
    b.w 975f
    .p2align 2
8:
    .word 0x9a5d892c
    .word 0x4d94113b
    .ltorg
975:
    umaal r10, r5, r5, r12
    rbit r2, r4
    adds.n r5, r3, #5
    usub16 r3, lr, r5
    dsb
    cpsie f
    mrs r5, msp
    sub.w r5, r5, #0x800
    msr psp, r5
    mov.w r5, #2
    msr control, r5
    isb
    ldr.w r5, =0xe000ed04
    ldr.w r4, =0x10000000
    str.w r4, [r5]
    mov.w r5, #0
    msr control, r5
    isb
    addw r12, sp, #3680
    add.n r2, sp, #956
    pop.n {r3, r5}
    cbz r5, 976f
    adds.n r2, r6, r2
976:
    sdiv r8, r4, r10
    sev.w
    cpsie f
    svc #138
    add.n r12, r8
    nop.w
    sub.n sp, sp, #240
    cpsie f
    mrs r5, msp
    sub.w r5, r5, #0x800
    msr psp, r5
    mov.w r5, #2
    msr control, r5
    isb
    ldr.w r5, =0xe000e200
    ldr.w r4, =0x00000008
    str.w r4, [r5]
    mov.w r5, #0
    msr control, r5
    isb
    orr.w r3, lr, #0x3580000
    sxtah r10, r8, r8, ror #16
    ldr.n r2, =0xe213ade5
    mul.w r11, r1, r7
    smlatt r10, r7, r10, r0
    it lt
    blt.w .Lg103
    orrs.w r0, r0, #0x570057
.Lg103:
    itt le
    qaddle r10, r10, r4
    ble.w .Lg104
    add.n sp, sp, #396
.Lg104:
    cmp.n r0, r1
    smuad r3, r6, r11
    ldr.w r4, =.Lg105+1
    push.w {r4}
    ldr.w pc, [sp], #4
    cmn.n r1, r3
.Lg105:
    ite ne
    lsrsne.w r4, r5, #22
    eoreq.w r4, r5, #0xf60000
    movw r10, #0x75f3
    cpsie f
    mov.w r5, #0x30000000
    ldrb.w r4, [r5, #3]
    mov.n r11, r8
    and.w r5, r4, #3
    tbb [pc, r5]
.Lg106:
        .byte (.Lg107-.Lg106)/2, (.Lg108-.Lg106)/2, (.Lg109-.Lg106)/2, (.Lg110-.Lg106)/2
.p2align 1
.Lg107:
        add.n r0, sp, #932
.Lg108:
        sub.w r1, r6, #0x22222222
.Lg109:
        add.n r3, sp, #912
.Lg110:
        cmp.w sp, #0x180018
    rsbs.n r0, r3, #0
    pkhbt r4, r8, r6, lsl #12
    ittee mi
    strmi.n r6, [r7, #92]
    smmulmi r0, r9, r4
    subspl.w r10, r4, #0x1b1b1b1b
    rev16pl.n r1, r4
    b.w 977f
    .p2align 2
8:
    .word 0x8a555d38
    .word 0xb0236da5
    .ltorg
977:
    and.w r5, r2, #3
    tbb [pc, r5]
.Lg111:
        .byte (.Lg112-.Lg111)/2, (.Lg113-.Lg111)/2, (.Lg114-.Lg111)/2, (.Lg115-.Lg111)/2
.p2align 1
.Lg112:
        sbcs.n r0, r3
.Lg113:
        qdadd r8, r7, r6
.Lg114:
        bfi r12, r9, #0, #24
.Lg115:
        subs.n r2, #180
    rev.w r5, r7
    strd r0, r10, [r7, #-996]
    mul.w r2, r11, r0
    stmia.w r7, {r1, r9}
    str.n r7, [r7, #88]
    addw r5, r7, #784
    ldrexh r12, [r5]
    clrex
    strexh r12, r9, [r5]
    cpsie f
    sub.w sp, sp, #4
    svc #219
    add.w sp, sp, #4
    mov.w r5, r7
    stmia.w r5!, {r0, r1, r4, r11}
    rev.n r2, r3
    adr.n r4, 8f
    pkhbt r9, r8, lr
    smlsd r2, r6, r1, r2
    muls.n r1, r5, r1
    ldr.n r4, =0xe82536bd
    clz r5, r6
    stmia.w r7, {r0, r2, r11, r12}
    b.w .Lg116
.Lg117:
    lsls.w r11, lr, r10
    b.w .Lg118
.Lg116:
    add.n r1, lr
    bge.w .Lg117
.Lg118:
    orn.w r12, r8, r7, asr #25
    lsrs.n r5, r4, #32
    add.n r10, r11
    adds.n r4, r0, r6
    mov.n r4, r9
    cbz r4, 978f
    muls.n r3, r6, r3
978:
    ldmdb.w r7, {r1, r8, r9, r10, r11}
    mov.n r3, r10
    mrs r12, eapsr
    cbnz r3, 979f
    ldr.n r4, [r7, r6]
    cmp.n r1, r5
979:
    adds.n r2, r4, r3
    subs.n r1, r7, #2
    adds.n r1, r0, r7
    cbz r1, 980f
    lsrs.n r2, r4
    cmp.n r8, r7
980:
    lsls.n r4, r0, #25
    stmia.w r7, {r4, r8, r10, r12}
    msr apsr_g, r7
    smlad r8, r4, r1, lr
    bmi.w .Lg119
    add.n r4, sp, #256
    eor.w r4, r6, #0x1380
.Lg119:
    smmla r8, r6, r5, r9
    movw r10, #0xd8c1
    movt r10, #0xc000
    movw r3, #0x7fff
    movt r3, #0x4000
    movw r8, #0x7fff
    movt r8, #0x8000
    mrs r5, apsr
    bic.w r5, r5, #0x08000000
    msr apsr_nzcvq, r5
    smlad r11, r10, r3, r8
    mov.w r5, r7
    stmia.w r5!, {r0, r1, r3, r8, r9}
    tst.n r7, r4
    usax r11, r12, r10
    pop.n {r1, r2, r3}
    add.n r4, sp, #64
    bne.w .Lg120
    adcs.w r12, r10, r12, asr #12
    sub.n sp, sp, #356
    usat r2, #20, r0, asr #25
.Lg120:
    b.w 981f
    .p2align 2
8:
    .word 0x83c9f00b
    .word 0xa2c77441
    .ltorg
981:
    str.n r2, [r7, #8]
    orns.w r8, r5, #0xfa00fa00
    mov.w r5, r7
    strd r9, r10, [r5], #784
    and.w r5, r2, #3
    tbb [pc, r5]
.Lg121:
        .byte (.Lg122-.Lg121)/2, (.Lg123-.Lg121)/2, (.Lg124-.Lg121)/2, (.Lg125-.Lg121)/2
.p2align 1
.Lg122:
        subw r8, r6, #1112
.Lg123:
        rsbs.n r4, r1, #0
.Lg124:
        tst.w r6, #0x1500000
.Lg125:
        mls r2, r6, lr, r12
    sub.w r11, sp, #0x96969696
    str.n r2, [r7, #4]
    addw r5, r7, #434
    ldrexh r4, [r5]
    strexh r9, r11, [r5]
    udiv r4, r4, r12
    mrs r5, msp
    sub.w r5, r5, #0x800
    msr psp, r5
    mov.w r5, #2
    msr control, r5
    isb
    push.w {r0, r1}
    pop.w {r2, r3}
    muls.n r2, r2, r2
    mov.w r5, #0
    msr control, r5
    isb
    adds.n r5, r5, r4
    teq.w r3, #0xa900a900
    ssub16 r4, r0, lr
    ldr.w r9, [sp, #3713]
    lsls.n r0, r5, #18
    mov.w r5, r7
    ldrsh.w r2, [r5], #64
    cbnz r1, 982f
    lsrs.n r0, r7, #19
    rsbs.n r3, r2, #0
    lsls.n r5, r7, #0
    stmia.n r5!, {r0}
    cmp.n r8, r6
    mov.n r9, r1
    nop.n
982:
    ldr.w r4, =.Lg126+1
    str.w r4, [r7, #720]
    ldr.w pc, [r7, #720]
    subs.n r4, r6, r2
.Lg126:
    mov.w r5, r7
    ldrsh.w r3, [r5], #203
    ite vs
    strbvs.n r3, [r7, #20]
    strvc.w lr, [sp, #967]
    lsls.n r0, r6
    nop.n
    addw r5, r6, #2837
    b.n 983f
    add.n r2, sp, #680
983:
    sbc.w r11, r3, r7, asr #28
    mov.w r5, r7
    ldrd r10, r3, [r5], #796
    bge.n 984f
    lsls.n r5, r7, #0
    ldmia.n r5!, {r0, r2, r3}
    pop.n {r0}
984:
    cpsie f
    udf.w #44695
    addw r5, r7, #512
    ldrexh r11, [r5]
    rsbs.n r0, r0, #0
    clrex
    strexh r3, r9, [r5]
    str.w r12, [sp, #2507]
    bfi r3, r8, #5, #18
    bpl.n 985f
    adds.n r1, #111
    adds.n r3, r2, #6
985:
    it cs
    movwcs r10, #0x4fcf
    rors.w r9, lr, r11
    addw r5, r7, #386
    ldrexh r9, [r5]
    yield.w
    pkhbt r8, lr, r0, lsl #14
    clrex
    strexh r2, r12, [r5]
    b.w 986f
    .p2align 2
8:
    .word 0xcbd50d5a
    .word 0xbb5b476a
    .ltorg
986:
    cpsie f
    mov.w r5, #0x30000000
    ldr.w r4, [r5]
    add.w sp, sp, #16
    muls.n r3, r2, r3
    and.w r5, r3, #3
    tbb [pc, r5]
.Lg127:
        .byte (.Lg128-.Lg127)/2, (.Lg129-.Lg127)/2, (.Lg130-.Lg127)/2, (.Lg131-.Lg127)/2
.p2align 1
.Lg128:
        smlalbt r8, r0, r8, lr
.Lg129:
        uqsub16 r9, r10, lr
.Lg130:
        add.n r11, r9
.Lg131:
        add.n r4, r12
    rsbs.n r0, r0, #0
    cpsie f
    add.w r5, r7, #3
    ldm.w r5, {r0, r1}
    push.n {r0, r3, r7}
    b.n 987f
    lsls.n r5, r7, #0
    stmia.n r5!, {r1, r2}
    adds.n r5, r4, r4
    cmp.n r9, r5
    movs.n r4, #64
    adds.n r0, r7, #5
    tst.n r4, r1
987:
    lsls.n r1, r3, #30
    orns.w r8, r3, #0xb1
    mov.n r10, r9
    movt r10, #0x5d80
    lsrs.w r11, r8, r9
    mvns.n r3, r3
    rsb.w r10, r7, #0xc500c5
    ldrb.n r3, [r7, r6]
    pop.n {r0}
    mov.w r5, r7
    strh.w r2, [r5, #68]!
    ite gt
    ldrgt.w r0, [sp, #1373]
    addle.n r8, r7
    cbz r0, 988f
    add.n r3, pc
988:
    pop.w {r2, r3, r8, r9}
    ssat r8, #2, r12, lsl #15
    addw r5, r7, #262
    ldrexh r8, [r5]
    smlsld r0, r2, r4, r11
    strexh r1, r9, [r5]
    cpsie f
    sub.w sp, sp, #4
    ldr.w r5, =0xe000ef00
    ldr.w r4, =0x00000001
    str.w r4, [r5]
    add.w sp, sp, #4
    add.n r0, r9
    movt r4, #0x97bb
    bcs.w .Lg132
    sev.w
    asrs.n r4, r2
    adds.n r4, #101
.Lg132:
    itet gt
    smlsldxgt r11, r4, r9, r0
    sxtble.n r0, r3
    nopgt.n
    usad8 r10, r10, r2
    itt cc
    qsubcc r12, lr, r10
    bcc.w .Lg133
    tst.w r9, #0x2100
.Lg133:
    ldr.n r5, [r7, #108]
    cbnz r0, 989f
    ldr.n r0, [r7, r6]
    ldrb.n r5, [r7, #6]
989:
    cbz r2, 990f
    nop.n
    ldr.n r2, [sp, #972]
    lsrs.n r2, r1, #22
    ldr.n r3, =0xddcf7f69
    nop.n
990:
    mov.n r8, r3
    push.w {r0, r2, r10, r11}
    ittte lt
    lsllt.w r3, r10, r2
    poplt.w {r0}
    noplt.n
    popge.n {r5}
    b.w 991f
    .p2align 2
8:
    .word 0x5a802687
    .word 0xc1a32902
    .ltorg
991:
    movw r10, #0x8000
    movt r10, #0x8000
    movw r1, #0x7fff
    movt r1, #0xc000
    movw r4, #0xc13e
    movt r4, #0x7ff0
    mrs r5, apsr
    bic.w r5, r5, #0x08000000
    msr apsr_nzcvq, r5
    smlabt r11, r10, r1, r4
    mvn.w r10, #0x9090909
    asrs.n r4, r4
    mrs r5, msp
    sub.w r5, r5, #256
    msr msp, r5
    adds.n r0, r7, r6
    msr basepri, r8
    cmp.w r8, r6, lsl #14
    addw r5, r7, #916
    ldrexb r11, [r5]
    sbcs.w r3, lr, #0xe800e800
    nop.n
    strexb r8, r9, [r5]
    dmb
    smmls r8, r2, r1, r5
    addw r12, lr, #2953
    lsls.n r5, r7, #0
    stmia.n r5!, {r0, r1, r3, r4}
    cpsie f
    udf.w #55400
    usad8 r9, r10, r11
    lsl.w r11, lr, r1
    add.w r12, lr, #0xd7d7d7d7
    itte ge
    movsge.w r1, #0x1e400
    mvnge.w r12, #0x91919191
    movwlt r9, #0x403
    movs.n r3, #255
    msr apsr_g, r3
    push.n {r1, r2, r3, r7, lr}
    sdiv r5, r0, r10
    adr.w r5, 8f
    orr.w r1, r6, #0xc8c8c8c8
    add.n r0, sp, #268
    teq.w r3, #0x70000000
    strh.w r7, [r7, r6, lsl #0]
    b.w .Lg134
    bics.n r3, r2
    add.n sp, sp, #448
.Lg134:
    cpsie f
    mov.w r5, #0
    sdiv r4, r0, r5
    itt mi
    strmi.n r4, [r7, #96]
    lslmi.n r0, r7, #18
    ssat16 r2, #15, r9
    pop.w {r10}
    ldr.w r11, [sp, #1636]
    strb.w r9, [r7, #-242]
    muls.n r5, r3, r5
    b.w .Lg135
.Lg136:
    mov.n r3, lr
    b.w .Lg137
.Lg135:
    rsbs.w r11, r4, #0x51
    b.w .Lg136
.Lg137:
    b.w .Lg138
.Lg139:
    adds.w r10, r10, #0xe600e6
    b.w .Lg140
.Lg138:
    sub.n sp, sp, #44
    bl .Lg139
.Lg140:
    cbnz r4, 992f
    adds.n r1, r5, r6
992:
    it mi
    bmi.w .Lg141
    orns.w r3, r1, #0x3e
.Lg141:
    itett mi
    uqsub8mi r3, r8, r7
    smmulrpl r2, r9, r3
    smladmi r2, r11, r11, r1
    smusdxmi r0, r2, r0
    smlalbb r0, r12, r10, r11
    b.w 993f
    .p2align 2
8:
    .word 0x894911b6
    .word 0xc18a0752
    .ltorg
993:
    orn.w r10, r3, #0x88
    subs.w r4, r5, lr, rrx
    and.w r5, r1, #3
    tbh [pc, r5, lsl #1]
.Lg142:
        .hword (.Lg143-.Lg142)/2, (.Lg144-.Lg142)/2, (.Lg145-.Lg142)/2, (.Lg146-.Lg142)/2
.p2align 1
.Lg143:
        addw r12, lr, #3188
.Lg144:
        uxtb16 r8, r4
.Lg145:
        nop.n
.Lg146:
        cmp.n r6, #109
    lsrs.n r3, r5, #27
    adds.n r3, r3, r4
    sub.n sp, sp, #428
    cbnz r0, 994f
    add.n r1, r12
    lsls.n r5, r7, #0
    ldmia.n r5!, {r1, r2}
    movs.n r1, #230
    subs.n r4, r3, #3
994:
    b.w .Lg147
.Lg148:
    add.n r1, sp
    b.w .Lg149
.Lg147:
    cmp.n r12, r6
    bvc.w .Lg148
.Lg149:
    pkhbt r9, r11, r1, lsl #27
    smlaldx r4, r2, r9, r12
    subs.n r4, r5, #5
    b.w .Lg150
.Lg151:
    movs.n r2, #171
    b.w .Lg152
.Lg150:
    nop.n
    bvs.w .Lg151
.Lg152:
    sub.n sp, sp, #276
    movw r12, #0x1921
    movt r12, #0x8000
    movw r11, #0xfb69
    movt r11, #0xc000
    movw r3, #0xcbf4
    movt r3, #0x7fff
    mrs r5, apsr
    bic.w r5, r5, #0x08000000
    msr apsr_nzcvq, r5
    smuad r2, r12, r11
    ldrh.n r0, [r7, r6]
    cmp.n r9, r2
    iteet ne
    ldrne.n r2, =0x9b7c5c79
    ldrbeq.n r2, [r7, #21]
    roreq.w r4, r1, r7
    sbcne.w r11, r7, #0x1ca0
    cbnz r3, 995f
    rsbs.n r1, r4, #0
995:
    bl .Lg153
    cmp.n r2, r7
.Lg153:
    cbnz r3, 996f
    cmp.n r10, r1
    add.n r1, sp, #252
996:
    itt ne
    cmpne.n r8, r4
    movne.n r3, lr
    msr apsr_nzcvqg, r10
    rsbs.n r0, r6, #0
    ldr.w r4, =.Lg154+1
    push.w {r4}
    ldr.w pc, [sp], #4
    asrs.n r0, r2, #8
.Lg154:
    subs.n r2, #211
    adr.n r5, 8f
    b.n 997f
    lsls.n r5, r7, #0
    ldmia.n r5!, {r0, r3, r4}
    asrs.n r3, r5, #6
    strh.n r0, [r7, #4]
    lsls.n r1, r5, #12
    lsls.n r2, r7, #7
997:
    rev.n r4, r4
    lsls.n r2, r2, #30
    udiv r2, r4, lr
    mls r5, r1, r2, r11
    adds.n r2, r7, r4
    mov.w r5, r7
    stmdb.w r5!, {r2, r3, r4, r9, r11}
    pop.n {r1, r4, r5}
    b.w 998f
    .p2align 2
8:
    .word 0x3f248d56
    .word 0x28bf079d
    .ltorg
998:
    pop.w {r9}
    subw sp, sp, #2032
    stmdb.w r7, {r1, r8}
    lsls.n r2, r5, #31
    lsls.n r0, r7, #30
    cpsie f
    ite le
    svcle #43
    uxtb16gt r8, r1, ror #16
    stmdb.w r7, {r3, r8, r10, r12}
    it cs
    smuadxcs r0, r6, lr
    msr psp, r10
    and.w r5, r3, #3
    tbh [pc, r5, lsl #1]
.Lg155:
        .hword (.Lg156-.Lg155)/2, (.Lg157-.Lg155)/2, (.Lg158-.Lg155)/2, (.Lg159-.Lg155)/2
.p2align 1
.Lg156:
        smlawt r8, r9, r12, r2
.Lg157:
        lsrs.n r0, r0, #18
.Lg158:
        add.n r4, r10
.Lg159:
        mov.n r12, r0
    lsls.n r4, r2, #18
    pkhtb r5, r2, r10, asr #22
    itte cs
    pkhtbcs r2, r8, r5, asr #1
    addcs.n r4, #73
    ldrsbcc.n r2, [r7, r6]
    qadd r2, r2, r5
    b.w .Lg160
    add.n sp, sp, #408
.Lg160:
    ldrd r3, r5, 8b
    mul.w r10, r12, r10
    lsrs.w r9, r3, r8
    subw r8, r1, #808
    adds.n r3, r3, r2
    ite hi
    cmphi.n r0, #32
    rsbls.n r1, r5, #0
    ldmdb.w r7, {r2, r4, r5}
    mls r11, r8, r8, r1
    asrs.n r1, r6, #14
    ldr.w r5, =0xe000ed20
    ldr.w r4, =0xebe039d4
    str.w r4, [r5]
    ldr.n r0, =0xd19c8ec5
    mov.w r5, r7
    ldrd r3, r1, [r5], #-560
    ldr.n r5, [sp, #364]
    it gt
    movgt.n r12, r0
    ldrh.n r4, [r7, #44]
    qsub16 r10, r11, r4
    eor.w r5, r5, #0x8a008a00
    umull r4, r11, r8, r8
    rsbs.n r2, r5, #0
    it vs
    addvs.w r12, r6, #0x2a
    sel r10, r8, r7
    ldr.n r1, =0x853b6e6d
    smlaltb r1, r5, r1, lr
    lsrs.n r2, r5, #24
    add.n sp, sp, #228
    ldr.w r5, =0xe000ed04
    ldr.w r4, =0x80000000
    str.w r4, [r5]
    subs.n r1, r4, r7
    b.w .Lg161
.Lg162:
    uhasx r2, r10, r10
    b.w .Lg163
.Lg161:
    nop.w
    bl .Lg162
.Lg163:
    sbc.w r8, r9, #0xcacacaca
    ble.w .Lg164
    add.n sp, sp, #444
    usub8 r8, r7, r3
.Lg164:
    asrs.n r5, r6, #6
    b.n 999f
    strh.n r4, [r7, r6]
    nop.n
    pop.n {r2}
999:
    b.n 9100f
    subs.n r1, #184
    ldr.n r3, =0x93900a9d
    tst.n r2, r7
    adds.n r1, r2, r7
    muls.n r1, r3, r1
    muls.n r0, r2, r0
9100:
    b.w 9101f
    .p2align 2
8:
    .word 0x8e89d310
    .word 0x0a24813c
    .ltorg
9101:
    eors.n r3, r6
    smlalbt r2, r1, r4, r5
    ldrsht r11, [r7, #193]
    ldrh.n r3, [r7, #4]
    ror.w r10, r4, r10
    mvns.n r0, r6
    b.w .Lg165
.Lg166:
    adc.w r12, r12, #0x65006500
    b.w .Lg167
.Lg165:
    cmp.n r2, r6
    bcc.w .Lg166
.Lg167:
    movs.n r4, #52
    rsbs.n r1, r5, #0
    add.n sp, sp, #452
    mov.w r5, r7
    ldmia.w r5!, {r0, r3, r8}
    teq.w r7, r5, ror #14
    add.n sp, sp, #404
    add.n sp, sp, #496
    cmp.n r8, r7
    add.n r0, sp, #348
    and.w r5, r2, #3
    tbb [pc, r5]
.Lg168:
        .byte (.Lg169-.Lg168)/2, (.Lg170-.Lg168)/2, (.Lg171-.Lg168)/2, (.Lg172-.Lg168)/2
.p2align 1
.Lg169:
        adds.n r4, #160
.Lg170:
        rsbs.n r4, r3, #0
.Lg171:
        ssub16 r9, r7, r8
.Lg172:
        umlal r0, r4, r9, r7
    ldr.w r5, =0xe000ed20
    ldr.w r4, =0x9b7a6c35
    str.w r4, [r5]
    subs.w r0, r7, #0x72727272
    muls.n r4, r3, r4
    cbz r4, 9102f
    adds.n r1, r7, r4
    ldr.n r0, [r7, #72]
    nop.n
    lsls.n r3, r0, #31
    strb.n r0, [r7, r6]
9102:
    addw r5, r7, #494
    ldrexh r4, [r5]
    cmp.n r9, r7
    strexh r9, r10, [r5]
    tst.n r3, r0
    adr.n r5, 8f
    pkhbt r11, r6, r6
    cmn.n r7, r4
    b.n 9103f
    ldrh.n r5, [r7, r6]
    ands.n r5, r6
    pop.n {r0}
9103:
    ldr.w r4, =.Lg173+1
    str.w r4, [r7, #248]
    add.w r5, r7, #244
    ldm.w r5, {r3, pc}
    subs.n r0, #157
.Lg173:
    mul.w r0, r8, r12
    mov.n r5, lr
    ittt le
    strble.n r1, [r7, #27]
    pkhbtle r11, r3, r1
    msrle apsr_nzcvqg, r3
    dsb
    adds.n r4, #103
    mov.w r5, r7
    stmia.w r5!, {r4, r12, lr}
    ble.n 9104f
    add.n r2, sp, #256
9104:
    itt le
    movwle r12, #0x2f05
    blle .Lg174
    cmp.n r10, r6
.Lg174:
    cmp.n r8, r6
    udiv r1, r7, r8
    cmp.n r7, #4
    mul.w r11, lr, r4
    mul.w r3, r5, r10
    cpsie f
    mrs r5, msp
    sub.w r5, r5, #0x800
    msr psp, r5
    mov.w r5, #2
    msr control, r5
    isb
    ldr.w r5, =0xe000e200
    ldr.w r4, =0x00000020
    str.w r4, [r5]
    mov.w r5, #0
    msr control, r5
    isb
    b.w 9105f
    .p2align 2
8:
    .word 0x8bd7f596
    .word 0xb661a181
    .ltorg
9105:
    movw r11, #0x8000
    movt r11, #0x7fff
    movw r12, #0x7fff
    movt r12, #0x8000
    movw r3, #0x10fd
    movt r3, #0x800f
    mrs r5, apsr
    bic.w r5, r5, #0x08000000
    msr apsr_nzcvq, r5
    smladx r1, r11, r12, r3
    and.w r10, r7, #0x5780000
    pld [r7, #-103]
    mov.w r5, r7
    strb.w r4, [r5], #-154
    it le
    movtle r8, #0x8ad4
    ittt cs
    cmpcs.n r12, r2
    movtcs r3, #0xb073
    uxtahcs r0, lr, r2
    stmia.w r7, {r3, r9, r10, r12}
    pop.n {r4}
    cmp.w r9, #0x8f008f00
    movs.n r2, #197
    mls r0, lr, r5, lr
    it ge
    bicge.w r12, r12, #0x9e009e
    adds.n r3, r2, r4
    mov.w r5, r7
    ldrh.w r0, [r5], #6
    ldr.w r5, =0xe000ed20
    ldr.w r4, =0xf71ceb93
    str.w r4, [r5]
    ldr.w r1, [sp, #1716]
    itt cc
    eorcc.w r0, r0, #0x5a
    bcc.w .Lg175
    movw r12, #0x2318
.Lg175:
    ittt ge
    ldrhge.n r4, [r7, #2]
    ldrshge.n r2, [r7, r6]
    mvnge.w r5, #0xad00ad00
    mrs r10, msp
    b.w .Lg176
.Lg177:
    ssat16 r11, #3, r8
    b.w .Lg178
.Lg176:
    lsrs.w r8, r8, r9
    bvc.w .Lg177
.Lg178:
    adds.n r2, r3, r0
    bics.w r0, r0, #0xc7c7c7c7
    subs.n r5, #170
    bic.w r0, r4, #0xf2
    ldr.w r4, =.Lg179+1
    push.w {r3, r4}
    pop.w {r2, pc}
    rsb.w r3, r1, #0x84
.Lg179:
    add.n r2, lr
    adr.w r8, 8f
    lsls.n r5, r7, #0
    stmia.n r5!, {r3}
    usat16 r11, #14, r5
    bpl.n 9106f
    strh.n r3, [r7, #52]
    ldrsb.n r2, [r7, r6]
    cmp.n r3, #186
    lsls.n r1, r7, #29
    lsrs.n r0, r0, #16
    rors.n r5, r3
9106:
    cpsie f
    ite ls
    svcls #68
    nophi.n
    cpsie f
    add.w r5, r7, #3
    strd r2, r3, [r5]
    add.n sp, sp, #496
    lsrs.w r1, r12, r6
    cmp.w sp, #0x9d009d
    ldr.w r4, =.Lg180+1
    push.w {r3, r4}
    pop.w {r2, pc}
    subw r3, r3, #325
.Lg180:
    b.w 9107f
    .p2align 2
8:
    .word 0x54ff7f0b
    .word 0x441da96c
    .ltorg
9107:
    mov.w r5, r7
    ldr.w r4, [r5, #-201]!
    cpsie f
    svc #28
    ldr.w r4, [r7, #-48]
    cbnz r1, 9108f
    lsls.n r5, r7, #0
    ldmia.n r5!, {r0, r1, r2, r3}
9108:
    subs.w r4, sp, r7, asr #2
    muls.n r4, r6, r4
    itt pl
    smladxpl r4, r4, r4, r2
    blpl .Lg181
    add.n r2, sp, #12
.Lg181:
    it vs
    subvs.w r0, r1, #0x5c00
    adds.n r0, r2, r7
    push.w {r3, r6, r11, lr}
    smlatb r11, r3, r12, r7
    smlaltt r11, r2, r9, r10
    adr.n r1, 8f
    and.w r5, r1, #3
    tbh [pc, r5, lsl #1]
.Lg182:
        .hword (.Lg183-.Lg182)/2, (.Lg184-.Lg182)/2, (.Lg185-.Lg182)/2, (.Lg186-.Lg182)/2
.p2align 1
.Lg183:
        adcs.w r1, r2, #0xd000d000
.Lg184:
        sel r8, r2, r6
.Lg185:
        ands.n r3, r0
.Lg186:
        sxtab16 r10, r10, r7, ror #24
    and.w r4, r5, #0x71
    iteet eq
    uhasxeq r5, r12, r0
    uxthne.w r11, r8, ror #16
    cmpne.n r11, r5
    cmneq.n r1, r2
    subs.n r0, r0, #1
    itttt cc
    ldrcc.n r4, =0x90475469
    ldrcc.n r2, =0xe6aaa5d9
    nopcc.n
    adccc.n r3, r4
    ldr.w r5, =0xe000e400
    ldr.w r4, =0x8380075e
    str.w r4, [r5]
    cmp.n r12, r2
    cbnz r0, 9109f
    ldrsh.n r2, [r7, r6]
    strh.n r2, [r7, #60]
    cmp.n r6, #104
9109:
    mov.n r5, lr
    mov.w r5, r7
    ldmdb.w r5!, {r0, r1, r9, r12}
    msr basepri_max, r11
    tst.w r12, r9, ror #4
    smultb r11, r11, r4
    smulwb r2, lr, r0
    bfi r1, r7, #19, #4
    push.n {r1, r6}
    msr apsr_g, r4
    cpsie f
    mrs r5, msp
    sub.w r5, r5, #0x800
    msr psp, r5
    mov.w r5, #2
    msr control, r5
    isb
    svc #0
    svc #78
    mov.w r5, #0
    msr control, r5
    isb
    sxtab r2, r1, r1, ror #16
    cpsie f
    mov.w r5, #0x30000000
    ldrb.w r4, [r5, #3]
    b.w .Lg187
.Lg188:
    cmp.n r10, r2
    b.w .Lg189
.Lg187:
    mov.w r1, #0xc8c8c8c8
    b.w .Lg188
.Lg189:
    str.w r6, [sp, #217]
    nop.n
    b.w 9110f
    .p2align 2
8:
    .word 0xb766ead8
    .word 0x25b19498
    .ltorg
9110:
    revsh.n r1, r6
    bcc.n 9111f
    lsls.n r5, r7, #0
    ldmia.n r5!, {r1, r2, r3, r4}
    sub.n sp, sp, #372
    adds.n r1, r0, #4
    sub.n sp, sp, #400
    strb.n r0, [r7, r6]
9111:
    add.n sp, sp, #60
    itete pl
    adrpl.w r2, 8b
    cmpmi.n r7, #157
    pushpl.n {r3}
    teqmi.w r10, r10, lsr #1
    add.n r1, pc
    b.w .Lg190
.Lg191:
    movt r1, #0xab85
    b.w .Lg192
.Lg190:
    usat16 r12, #4, r11
    bl .Lg191
.Lg192:
    adr.w r12, 8b
    mrs r3, basepri_max
    and.w r5, r4, #3
    tbh [pc, r5, lsl #1]
.Lg193:
        .hword (.Lg194-.Lg193)/2, (.Lg195-.Lg193)/2, (.Lg196-.Lg193)/2, (.Lg197-.Lg193)/2
.p2align 1
.Lg194:
        subw r11, r8, #3977
.Lg195:
        bic.w r9, lr, #0x3f
.Lg196:
        add.n r0, sp, #400
.Lg197:
        add.n r8, r3
    itt ge
    movge.w r8, sp
    blge .Lg198
    mvns.w r12, r2, lsl #1
.Lg198:
    cpsie f
    udf.w #40117
    bvc.n 9112f
    lsrs.n r1, r5, #17
    nop.n
    sub.n sp, sp, #384
    muls.n r1, r4, r1
    lsls.n r5, r7, #0
    stmia.n r5!, {r0, r3}
    orrs.n r1, r1
9112:
    itttt cs
    popcs.w {r2}
    pkhbtcs r9, r6, r7
    shasxcs r8, r5, r9
    nopcs.n
    usat16 r3, #5, r2
    and.w r5, r1, #3
    tbh [pc, r5, lsl #1]
.Lg199:
        .hword (.Lg200-.Lg199)/2, (.Lg201-.Lg199)/2, (.Lg202-.Lg199)/2, (.Lg203-.Lg199)/2
.p2align 1
.Lg200:
        uhadd16 r3, r3, r0
.Lg201:
        smull r4, r11, lr, r0
.Lg202:
        cmp.n r2, #196
.Lg203:
        mov.n r12, r2
    asrs.n r1, r6, #15
    rev.n r3, r1
    smulwt r0, r0, r8
    mov.n r5, r12
    b.n 9113f
    nop.n
9113:
    adds.n r5, #40
    ittt vc
    sdivvc r9, r11, r8
    ldrshvc.n r0, [r7, r6]
    strbvc.n r4, [r7, r6]
    b.w .Lg204
.Lg205:
    add.n r4, lr
    b.w .Lg206
.Lg204:
    sbcs.w r4, r11, #0x19c00000
    bpl.w .Lg205
.Lg206:
    itee cs
    adrcs.w r5, 8f
    qasxcc r12, r2, r5
    addcc.n r0, #95
    nop.n
    addw r5, r7, #315
    ldrexb r9, [r5]
    strexb r1, r9, [r5]
    uxtab r9, r12, r11, ror #24
    ldr.n r0, [r7, r6]
    b.w 9114f
    .p2align 2
8:
    .word 0x92096879
    .word 0xfdc5589f
    .ltorg
9114:
    smultt r12, r10, r7
    smuadx r8, r3, r6
    mov.n r2, r12
    ldrb.n r4, [r7, r6]
    ldrd r8, r10, [r7, #128]
    pop.n {r1}
    addw r4, r3, #3150
    sxtah r1, r1, r11
    qadd16 r5, r7, r12
    mov.w r5, r7
    ldrh.w r9, [r5, #223]!
    bge.w .Lg207
    shadd8 r9, r3, r2
    cmp.n r8, r2
    umlal r8, r9, r10, r12
    sub.w r11, r2, #0xb8b8b8b8
.Lg207:
    adds.n r1, #225
    revsh.n r5, r5
    ssat16 r10, #10, r5
    lsls.n r1, r3, #8
    itt ne
    subwne r0, r4, #1894
    bne.w .Lg208
    cmp.n r0, #234
.Lg208:
    ite cs
    cmpcs.n r6, #120
    nopcc.n
    adr.n r5, 8f
    adds.n r2, #71
    sbfx r1, r3, #24, #3
    tst.w r3, #0x600060
    mov.w r5, r7
    ldrd r1, r12, [r5, #1004]!
    add.n r3, sp, #508
    cpsie f
    ite cc
    svccc #208
    cmncs.w r12, #0x31313131
    rev.n r3, r7
    lsls.n r5, r2, #6
    rrx.w r3, r3
    b.n 9115f
    rsbs.n r2, r4, #0
    rsbs.n r2, r1, #0
    adds.n r3, r3, r5
    push.n {r2, r7, lr}
    cmp.n r7, r2
    pop.n {r2, r3, r5}
9115:
    bics.w r5, lr, r3, asr #14
    ble.n 9116f
    lsls.n r1, r5, #24
    cmp.n r2, #228
9116:
    add.n sp, sp, #108
    itt pl
    msrpl primask, r2
    bpl.w .Lg209
    sel r0, r2, r8
.Lg209:
    add.n r5, pc
    subw sp, sp, #2712
    lsls.n r5, r7, #0
    stmia.n r5!, {r3}
    cbz r3, 9117f
    subs.n r4, #114
    strb.n r3, [r7, #23]
    asrs.n r2, r4, #27
    str.n r0, [r7, r6]
9117:
    mov.w r5, r7
    ldrd r1, r11, [r5, #932]!
    ldr.w r5, =0xe000e404
    ldr.w r1, [r5]
    movw r0, #0x7fff
    movt r0, #0xc000
    movw r4, #0x7fff
    movt r4, #0x8000
    movw r8, #0x7fff
    movt r8, #0x7ff0
    mrs r5, apsr
    bic.w r5, r5, #0x08000000
    msr apsr_nzcvq, r5
    usat r2, #8, r0
    push.n {r0, r2}
    b.w 9118f
    .p2align 2
8:
    .word 0x44a87e1e
    .word 0xc99d204c
    .ltorg
9118:
    msr apsr_g, r4
    muls.n r2, r5, r2
    push.w {r3, r8, r9, lr}
    movt r5, #0xb2bb
    sbc.w r4, r10, #0x32323232
    and.w r5, r4, #3
    tbh [pc, r5, lsl #1]
.Lg210:
        .hword (.Lg211-.Lg210)/2, (.Lg212-.Lg210)/2, (.Lg213-.Lg210)/2, (.Lg214-.Lg210)/2
.p2align 1
.Lg211:
        ubfx r12, r1, #1, #4
.Lg212:
        nop.n
.Lg213:
        cmp.n r11, r7
.Lg214:
        cmp.n r2, #125
    strb.n r0, [r7, r6]
    b.w .Lg215
.Lg216:
    add.n r10, lr
    b.w .Lg217
.Lg215:
    asrs.n r2, r4, #8
    bl .Lg216
.Lg217:
    adc.w r0, r11, r0, lsr #18
    lsls.n r2, r0, #16
    lsls.n r1, r1, #3
    b.n 9119f
    lsls.n r5, r7, #0
    ldmia.n r5!, {r0, r1, r4}
    muls.n r3, r5, r3
    mov.n r10, sp
    lsls.n r4, r0, #15
    pop.n {r3}
    sub.n sp, sp, #444
9119:
    b.n 9120f
    sub.n sp, sp, #116
    muls.n r1, r0, r1
    lsls.n r0, r4, #7
9120:
    addw r2, r2, #3298
    ldr.w r4, =.Lg218+1
    str.w r4, [r7, #292]
    add.w r5, r7, #288
    ldm.w r5, {r3, pc}
    nop.n
.Lg218:
    subw r9, r7, #18
    it pl
    bpl.w .Lg219
    subs.n r2, r1, r4
.Lg219:
    smlaltt r8, r9, r11, r9
    bvs.n 9121f
    mov.n r11, r2
    ldrsh.n r1, [r7, r6]
9121:
    add.n r3, sp, #380
    mov.w r5, r7
    stmdb.w r5!, {r1, r3, r4, r8, r10, r12}
    lsrs.n r1, r7, #3
    mla r11, r10, r10, r12
    b.n 9122f
    asrs.n r4, r0
    add.n r0, pc
    subs.n r4, #37
9122:
    itet ge
    addge.n r8, r6
    movlt.n r4, r11
    ssub16ge r5, r6, r1
    ldrsb.w r12, 8f
    smmlar r3, r0, r3, r7
    mov.w r5, r7
    str.w r4, [r5, #-240]!
    movs.n r4, #0x40
    msr basepri, r4
    ldr.w r5, =0xe000ed04
    ldr.w r4, =0x80000000
    str.w r4, [r5]
    bfc r10, #18, #5
    movs.n r4, #0
    msr basepri, r4
    lsr.w r11, r7, r2
    sub.w r2, sp, #0x4d004d00
    cmp.w r2, #0x62
    rsbs.n r3, r7, #0
    pop.n {r2, r4}
    push.n {r3, r5}
    ldr.w r4, =.Lg220+1
    push.w {r3, r4}
    pop.w {r2, pc}
    orn.w r2, r4, #0x930093
.Lg220:
    b.w 9123f
    .p2align 2
8:
    .word 0x698536f1
    .word 0x8932006d
    .ltorg
9123:
    sub.n sp, sp, #176
    lsls.n r1, r3, #17
    str.w r2, [sp, #3380]
    asr.w r3, r5, r8
    lsrs.n r1, r4, #2
    add.n r12, r9
    rsbs.w r3, r10, #0xb700
    lsls.n r1, r4, #11
    ldr.n r4, [sp, #628]
    b.w .Lg221
.Lg222:
    cmp.w r9, r1, lsr #24
    b.w .Lg223
.Lg221:
    mul.w r1, r2, r11
    b.w .Lg222
.Lg223:
    b.n 9124f
    add.n r5, sp, #760
    sub.n sp, sp, #384
    ldr.n r5, =0xde7ab46d
    ldr.n r5, [r7, r6]
    cmp.n r4, #154
9124:
    it ls
    asrls.n r2, r1, #25
    bl .Lg224
    bics.w r8, r12, lr, ror #31
    rrx.w r11, r0
    subw r0, r11, #2032
.Lg224:
    sel r10, r11, r2
    rev16.n r3, r4
    push.n {r0, r3, r7}
    sub.n sp, sp, #232
    pop.n {r1}
    ldr.w r5, =0xe000e180
    ldr.w r4, =0x00000040
    str.w r4, [r5]
    str.w r4, [r7, #-239]
    adcs.w r10, r9, r0, asr #5
    subs.n r4, #190
    mrs r5, msp
    sub.w r5, r5, #0x800
    msr psp, r5
    mov.w r5, #2
    msr control, r5
    isb
    push.w {r0, r1}
    pop.w {r2, r3}
    adds.n r2, r2, #2
    mov.w r5, #0
    msr control, r5
    isb
    teq.w r2, lr, lsl #20
    add.n sp, sp, #124
    cmp.n r7, r2
    cpsie f
    sub.w sp, sp, #4
    svc #112
    add.w sp, sp, #4
    uxtb.n r2, r5
    sub.n sp, sp, #148
    ldr.n r3, [sp, #860]
    ldr.w r5, =0xe000e100
    ldr.w r4, =0x00000080
    str.w r4, [r5]
    b.w .Lg225
.Lg226:
    cmp.n r11, r6
    b.w .Lg227
.Lg225:
    add.n r1, sp, #808
    bl .Lg226
.Lg227:
    smlaldx r8, r9, r7, r10
    push.n {r2, r3, r4, r7, lr}
    strbt r3, [r7, #232]
    b.n 9125f
    rev16.n r4, r5
    asrs.n r0, r4, #11
    str.n r4, [sp, #656]
    adds.n r2, r0, r1
    cmp.n r10, r4
9125:
    itt eq
    bicseq.w r1, r10, #0x7a7a7a7a
    cmpeq.n r4, #8
    muls.n r0, r3, r0
    b.w 9126f
    .p2align 2
8:
    .word 0x4d9f2e2d
    .word 0x80c83fbd
    .ltorg
9126:
    cpsie f
    mov.w r5, #0
    udiv r4, r0, r5
    mov.n r2, pc
    rsbs.n r3, r5, #0
    ldr.n r3, =0xd947a9e9
    cpsie f
    mov.w r5, #0x30000000
    ldrb.w r4, [r5, #3]
    mov.w r5, r7
    ldmia.w r5!, {r2, r3, r9, r11}
    it eq
    cmpeq.n r11, r0
    nop.n
    bfc r10, #30, #1
    b.w .Lg228
.Lg229:
    orns.w r11, r0, #0x64006400
    b.w .Lg230
.Lg228:
    add.n r3, r11
    bl .Lg229
.Lg230:
    tst.w r2, lr, lsr #11
    ldrex r1, [r7, #360]
    strex r8, r0, [r7, #360]
    cmn.w r8, r8, lsr #16
    msr psp, r4
    add.n r11, r3
    ssat r8, #13, r11, lsl #10
    cmp.n r9, r0
    mov.w r5, sp
    push.n {r4, r6}
    cpsie f
    add.w r5, r7, #2
    ldrex r0, [r5]
    ldrb.n r4, [r7, r6]
    rev16.n r5, r1
    pkhbt r8, r8, r5, lsl #18
    adds.n r2, r6, #5
    clz r10, lr
    b.w 9127f
    .p2align 2
8:
    .word 0x8badf00d
    .word 0x0ddba11
    .ltorg
9127:
    ldr r1, =0x20026
    movs r0, #0x18
    bkpt 0xab
    .ltorg
.thumb_func
h_fault:
    tst.w lr, #4
    ite eq
    mrseq r0, msp
    mrsne r0, psp
    ldr.w r3, =0xE000ED28
    ldr r1, [r3]
    str r1, [r3]
    ldr r2, [r3, #4]
    str r2, [r3, #4]
    ldr r1, [r0, #24]
    ldrh r2, [r1]
    lsrs r2, r2, #11
    cmp r2, #0x1d
    ite cs
    addcs r1, r1, #4
    addcc r1, r1, #2
    str r1, [r0, #24]
    ldr r2, [r0, #28]
    ubfx r3, r2, #25, #2
    ubfx r1, r2, #10, #6
    orr.w r3, r3, r1, lsl #2
    tst.w r3, #0xF
    beq .Lg232
    tst.w r3, #7
    beq .Lg233
    and.w r1, r3, #0x1F
    lsl.w r1, r1, #1
    and.w r1, r1, #0x1F
    bic.w r3, r3, #0x1F
    orr.w r3, r3, r1
    b .Lg234
    .Lg233:
    movs r3, #0
    .Lg234:
    bic.w r2, r2, #0x06000000
    bic.w r2, r2, #0x0000FC00
    and.w r1, r3, #3
    orr.w r2, r2, r1, lsl #25
    lsr.w r1, r3, #2
    orr.w r2, r2, r1, lsl #10
    str r2, [r0, #28]
    .Lg232:
    .Lg231:
    push {r4, lr}
    ldr.w r5, =0xe000ed18
    ldr.w r1, [r5]
    cmp.n r11, r0
    pop {r4, pc}
.thumb_func
h_hardfault:
    tst.w lr, #4
    ite eq
    mrseq r0, msp
    mrsne r0, psp
    ldr.w r3, =0xE000ED28
    ldr r1, [r3]
    str r1, [r3]
    ldr r2, [r3, #4]
    str r2, [r3, #4]
    cbz r1, .Lg235
    ldr r1, [r0, #24]
    ldrh r2, [r1]
    lsrs r2, r2, #11
    cmp r2, #0x1d
    ite cs
    addcs r1, r1, #4
    addcc r1, r1, #2
    str r1, [r0, #24]
    ldr r2, [r0, #28]
    ubfx r3, r2, #25, #2
    ubfx r1, r2, #10, #6
    orr.w r3, r3, r1, lsl #2
    tst.w r3, #0xF
    beq .Lg236
    tst.w r3, #7
    beq .Lg237
    and.w r1, r3, #0x1F
    lsl.w r1, r1, #1
    and.w r1, r1, #0x1F
    bic.w r3, r3, #0x1F
    orr.w r3, r3, r1
    b .Lg238
    .Lg237:
    movs r3, #0
    .Lg238:
    bic.w r2, r2, #0x06000000
    bic.w r2, r2, #0x0000FC00
    and.w r1, r3, #3
    orr.w r2, r2, r1, lsl #25
    lsr.w r1, r3, #2
    orr.w r2, r2, r1, lsl #10
    str r2, [r0, #28]
    .Lg236:
    .Lg235:
    ldr r1, [r0, #8]
    ldr r3, [r0, #4]
    push {r4, lr}
    uhsub8 r1, r3, r10
    pop {r4, pc}
.thumb_func
h_nmi:
    tst.w lr, #4
    ite eq
    mrseq r0, msp
    mrsne r0, psp
    push.w {lr}
    rev16.w r8, r4
    orr.w r9, r1, #0x55800
    mov.w r10, #0x8a8a8a8a
    ldr.w pc, [sp], #4
.thumb_func
h_svc:
    tst.w lr, #4
    ite eq
    mrseq r0, msp
    mrsne r0, psp
    ldr r3, [r0, #16]
    ldr r1, [r0, #8]
    adds r1, #209
    str r1, [r0, #8]
    push {r4, lr}
    pop {r4, pc}
.thumb_func
h_pendsv:
    tst.w lr, #4
    ite eq
    mrseq r0, msp
    mrsne r0, psp
    ldr r1, [r0, #20]
    push {r4, lr}
    ldr.w r5, =0xe000ed04
    ldr.w r4, =0x80000000
    str.w r4, [r5]
    uadd8 r3, r1, r3
    pop {r4, pc}
.thumb_func
h_systick:
    tst.w lr, #4
    ite eq
    mrseq r0, msp
    mrsne r0, psp
    ldr r1, [r0, #0]
    adds r1, #84
    str r1, [r0, #0]
    push {r4, lr}
    ldr.w r5, =0xe000ed04
    ldr.w r4, =0x10000000
    str.w r4, [r5]
    pop {r4, pc}
.thumb_func
h_irq0:
    tst.w lr, #4
    ite eq
    mrseq r0, msp
    mrsne r0, psp
    ldr r1, [r0, #12]
    ldr r3, [r0, #28]
    push {r4, lr}
    pop {r4, pc}
.thumb_func
h_irq1:
    tst.w lr, #4
    ite eq
    mrseq r0, msp
    mrsne r0, psp
    ldr r2, [r0, #12]
    ldr r2, [r0, #12]
    ldr r1, [r0, #4]
    adds r1, #58
    str r1, [r0, #4]
    push.w {lr}
    ldr.w pc, [sp], #4
.thumb_func
h_irq2:
    tst.w lr, #4
    ite eq
    mrseq r0, msp
    mrsne r0, psp
    ldr r3, [r0, #4]
    ldr r1, [r0, #4]
    tst.n r4, r4
    ldr.w r5, =0xe000ed2c
    ldr.w r1, [r5]
    bx lr
.thumb_func
h_irq3:
    tst.w lr, #4
    ite eq
    mrseq r0, msp
    mrsne r0, psp
    add.n r8, r7
    bx lr
.thumb_func
h_irq4:
    tst.w lr, #4
    ite eq
    mrseq r0, msp
    mrsne r0, psp
    ldr r1, [r0, #4]
    push.w {lr}
    ldr.w r5, =0xe000ed20
    ldr.w r2, [r5]
    muls.n r2, r1, r2
    ldr.w pc, [sp], #4
.thumb_func
h_irq5:
    tst.w lr, #4
    ite eq
    mrseq r0, msp
    mrsne r0, psp
    push.w {lr}
    ldr.w r5, =0xe000ed1c
    ldr.w r2, [r5]
    nop.n
    lsls.w r2, r10, #22
    ldr.w pc, [sp], #4
.thumb_func
h_irq6:
    tst.w lr, #4
    ite eq
    mrseq r0, msp
    mrsne r0, psp
    ldr r1, [r0, #0]
    ldr r1, [r0, #16]
    adds r1, #67
    str r1, [r0, #16]
    push.w {lr}
    cmp.n r6, #159
    nop.n
    ldr.w pc, [sp], #4
.thumb_func
h_irq7:
    tst.w lr, #4
    ite eq
    mrseq r0, msp
    mrsne r0, psp
    push {r4, lr}
    mrs r1, basepri
    pop {r4, pc}
.thumb_func
h_irqx:
    bkpt 0x01
.thumb_func
h_spare:
    bkpt 0x02
    .ltorg
.section .buf, "aw"
    .word 0x42d60baa
    .word 0x1b901e78
    .word 0x42ac030c
    .word 0x69701591
    .word 0x53ce530e
    .word 0xf7cf5a6c
    .word 0x0000019c
    .word 0xd5a262c8
    .word 0x1b8e8d8d
    .word 0xc5f6ffa8
    .word 0x5353f361
    .word 0xe5aefe75
    .word 0x4fa1f5f5
    .word 0x20300000
    .word 0x91b9b6a2
    .word 0x9e1b43fd
    .word 0x3549d871
    .word 0xa8e441c7
    .word 0x92152da9
    .word 0xed014724
    .word 0x91b4fbee
    .word 0xf7aaa972
    .word 0xdabff363
    .word 0x00039034
    .word 0x16ae4ff3
    .word 0xd9b7e4e0
    .word 0x2afc1d5d
    .word 0xc0cbfb2c
    .word 0x8d4a33be
    .word 0x7eca601f
    .word 0xf0e59c6f
    .word 0x1af4826f
    .word 0x1d3fc2c8
    .word 0xb74c072f
    .word 0x05b83220
    .word 0xa188533c
    .word 0x2fb08aba
    .word 0x8f666178
    .word 0xfde4e81a
    .word 0xba6c97dc
    .word 0x775ea0de
    .word 0xa2c35c5c
    .word 0xd8999fa7
    .word 0x68356075
    .word 0x4bd1bd36
    .word 0xa9fea4f6
    .word 0x82110492
    .word 0x0ed6fa24
    .word 0xfcb679cd
    .word 0x63fa14df
    .word 0xe0f48890
    .word 0x1cc6b270
    .word 0x2e5a43e1
    .word 0x2a1edfb5
    .word 0x13286da0
    .word 0xef13d894
    .word 0x7a481a2f
    .word 0xa872c7b0
    .word 0x4e1eebb4
    .word 0xecb8fabb
    .word 0xdafd7d48
    .word 0x44343606
    .word 0x92dabc33
    .word 0xdeb97360
    .word 0x657d7512
    .word 0x9b4e1caf
    .word 0xba73e484
    .word 0x02fe27c5
    .word 0x8a14bece
    .word 0x1cd91db8
    .word 0x8e44e55c
    .word 0x31b22d2f
    .word 0x4d59d08a
    .word 0x5919841f
    .word 0x6136b5cd
    .word 0x29f15a8a
    .word 0x57678cee
    .word 0xea932395
    .word 0x06dbcb90
    .word 0x15a5d92e
    .word 0xe75730b5
    .word 0x83cc69ab
    .word 0x07036cfe
    .word 0x81ecfa6d
    .word 0x8cbce36d
    .word 0x5e9495e1
    .word 0x2f38b41a
    .word 0xe2a65e01
    .word 0x24d88b17
    .word 0xf69f03e0
    .word 0x9dc7a9d9
    .word 0x3568cf76
    .word 0x10740857
    .word 0x8fd1a382
    .word 0xdef9eb91
    .word 0x85d8d23c
    .word 0x09dfeca2
    .word 0xf8a03882
    .word 0x093b1f46
    .word 0x4e9e20fc
    .word 0x5d238898
    .word 0x7644c439
    .word 0x092f7c6a
    .word 0xfb4be782
    .word 0xca3c40bb
    .word 0x96d53a89
    .word 0x4876f84f
    .word 0xc2ee8cba
    .word 0xa9252b72
    .word 0xcae86dbd
    .word 0x5433baf2
    .word 0xba9e6ea1
    .word 0x986db416
    .word 0x9a4b516c
    .word 0x578751d4
    .word 0x694b4b3f
    .word 0x84fc495e
    .word 0x2703a677
    .word 0x8e14a33c
    .word 0xd2bc5f14
    .word 0x2bc1cec8
    .word 0x88e9fea9
    .word 0x59744a3f
    .word 0x026c10fe
    .word 0x54d526ce
    .word 0x1fb56cb8
    .word 0x3ee70fcc
    .word 0xb86017b7
    .word 0xe894a3f9
    .word 0xa018c02d
    .word 0x0e0ad3cd
    .word 0x5aa7eab4
    .word 0x2c94a229
    .word 0x5307d487
    .word 0x43db953c
    .word 0xe38f64b6
    .word 0x8d93b4af
    .word 0x0bf27933
    .word 0x470e1ad7
    .word 0xc8b055bb
    .word 0xf90d8c7e
    .word 0xb235c622
    .word 0xf690286d
    .word 0x222da9f8
    .word 0x865c32a6
    .word 0xb31247d1
    .word 0xd259719c
    .word 0x299fbf5b
    .word 0x6753d55e
    .word 0x7316e3b1
    .word 0x740ad905
    .word 0x0f5fee6d
    .word 0x22bcb29d
    .word 0xa8e425d3
    .word 0x55e62dff
    .word 0x07fddc53
    .word 0x3a76bb41
    .word 0x35d7bf07
    .word 0x85e6c7bd
    .word 0xde2e50a1
    .word 0x0f580b5c
    .word 0x628bebf4
    .word 0x1cf6363a
    .word 0xce1f7e6e
    .word 0x4e08f24d
    .word 0x9489685c
    .word 0x31f33661
    .word 0xf675daab
    .word 0xc4217d2d
    .word 0x2569970e
    .word 0xf45a6dae
    .word 0xaad8cd01
    .word 0x0e0aa7b3
    .word 0x3e1332eb
    .word 0x96ed1681
    .word 0x7502d36d
    .word 0x47d860e9
    .word 0x1a0b3a99
    .word 0xd25f9df6
    .word 0xd1ad8b2a
    .word 0x43953e22
    .word 0xd5814e77
    .word 0xbb576451
    .word 0x4745b1e8
    .word 0x1c7be48d
    .word 0x20c937d8
    .word 0xe36b42f6
    .word 0xaca638b6
    .word 0x66d87968
    .word 0x288d2a95
    .word 0xfc360e14
    .word 0xa471015e
    .word 0x6ecbb509
    .word 0x711eb4bf
    .word 0xc2ffff16
    .word 0xa6b50019
    .word 0x92f75919
    .word 0xb29de2dd
    .word 0x68a0f49a
    .word 0x9ceeaa91
    .word 0x0b26b698
    .word 0x60c857df
    .word 0x4473c3b0
    .word 0x9336e739
    .word 0x6bfc232a
    .word 0xd21786c3
    .word 0xc95b455d
    .word 0xed1008f7
    .word 0xef62299b
    .word 0xb2b81d8d
    .word 0x7354167e
    .word 0x99b28c04
    .word 0x2d399d37
    .word 0xcbbe154b
    .word 0xaf3e41ed
    .word 0xf8f634ed
    .word 0xa8d28d4f
    .word 0x0adf11e6
    .word 0x94546edb
    .word 0x6748e53a
    .word 0xf2d86195
    .word 0x77b321a1
    .word 0x25f732f1
    .word 0xcb513aa4
    .word 0x0dcc9307
    .word 0x1290de4d
    .word 0x0954b17b
    .word 0x1a8e6a7e
    .word 0xde52d763
    .word 0x004226c8
    .word 0x4339c088
    .word 0xc6b40ca1
    .word 0x61e9a741
    .word 0x09efeea9
    .word 0xc126836a
    .word 0x358fb26f
    .word 0x3339203e
    .word 0x9b613a96
    .word 0x758e58a4
    .word 0x295d7bb5
    .word 0x0fc8ce3f
    .word 0x7c063f72
    .word 0xa7588d1a
    .word 0xf8c1a740
    .word 0x69207550
    .word 0xb605a441
    .word 0xc7815ef6
    .word 0xe572ea1c
    .word 0x5ae59969
    .word 0xe71df830
    .word 0xb12032e7
    .word 0xd1f6e8c6
    .word 0x9c062af6
    .word 0x59781232
    .word 0x7b29377e
    .word 0xe275bda0
    .word 0xcef961be
    .word 0xaa4cf173
    .word 0x3e42f500
    .word 0xb443d43f
    .word 0xc794dcec
    .word 0x63e2f3de
    .word 0x0eca3938
    .word 0x6880f2d5
    .word 0x6a2c74d8
    .word 0x4dac53b2
    .word 0xee15ce77
    .word 0x881a1e37
    .word 0x36de718c
    .word 0x682a11c6
    .word 0xde078f3a
    .word 0xc38c30b5
    .word 0x27a59e82
    .word 0xdb9628f8
    .word 0x048c0e34
    .word 0x787171e8
    .word 0x741fe0f3
    .word 0x87bb0d01
    .word 0x5072681d
    .word 0x730e4e31
    .word 0xfb2d92ff
    .word 0x96998396
    .word 0x3c484d14
    .word 0x15c8cc69
    .word 0xd58230af
    .word 0xf642914a
    .word 0xa7595027
    .word 0xccfea72e
    .word 0xddf015ae
    .word 0x8f9bb02e
    .word 0x7eb98756
    .word 0xd29d64c5
    .word 0x201f3835
    .word 0x8db619f7
    .word 0x7602c316
    .word 0x103335b5
    .word 0xba084750
    .word 0x8eab8583
    .word 0x02008f7e
    .word 0x9b33e574
    .word 0x4cb853a0
    .word 0x5b98267b
    .word 0x65a37a5c
    .word 0x89f9ca5a
    .word 0x905f4f77
    .word 0x649f677b
    .word 0x14e6a894
    .word 0xcd2570ee
    .word 0xac75dc0f
    .word 0x286d7a95
    .word 0xb43fe0eb
    .word 0x6f1b3d49
    .word 0x4c29aa0b
    .word 0x90fa4d9a
    .word 0xbdd21ee2
    .word 0x234c1fe3
    .word 0x6aaedad2
    .word 0x55e2f86d
    .word 0xb9831c42
    .word 0xb464fc05
    .word 0x85c742d7
    .word 0x53452720
    .word 0x663cc9b8
    .word 0x3a192128
    .word 0x1b8fd8ae
    .word 0xd30676c4
    .word 0x20ab7bac
    .word 0x4f1961a5
    .word 0x38366c6d
    .word 0x5f46d109
    .word 0xe5a99b66
    .word 0x1472def8
    .word 0x4ab4590e
    .word 0xfa67d007
    .word 0x0c0f2623
    .word 0xc571869d
    .word 0xd6f3cd65
    .word 0xe895df8d
    .word 0xcaab8540
    .word 0xdacd28bd
    .word 0x79c04a32
    .word 0x344c9b3e
    .word 0xb4958937
    .word 0x09b3a61a
    .word 0x66b2fe9d
    .word 0x5e9dc543
    .word 0x470e47a2
    .word 0x354edf11
    .word 0x3e579fc3
    .word 0x279c590e
    .word 0x88cff371
    .word 0xdcd4041e
    .word 0x164f28ff
    .word 0x7748321c
    .word 0xa1f18ef8
    .word 0x918ada75
    .word 0x1fc7f7fa
    .word 0x852c5f89
    .word 0x3039d1ff
    .word 0x2d7d413b
    .word 0x23cedeb3
    .word 0x4abe3033
    .word 0x853a21e9
    .word 0x0ce3d987
    .word 0x59d96c0f
    .word 0x36b8c271
    .word 0x8940db13
    .word 0x61cb16e1
    .word 0xd8cc78b9
    .word 0xf65c1200
    .word 0xf63db440
    .word 0x3270812f
    .word 0x20d8cfda
    .word 0xd6599b37
    .word 0x5880fc77
    .word 0xce8db5eb
    .word 0xf9fb3bc0
    .word 0x4674649a
    .word 0xd4afdfda
    .word 0xb02d8f64
    .word 0x8484dbf0
    .word 0xfa2b8974
    .word 0x4d09b501
    .word 0xf71976b8
    .word 0x0ed7aca2
    .word 0x26d49505
    .word 0x29f6ead3
    .word 0x43e95c07
    .word 0x3664ad4c
    .word 0xc3256c3e
    .word 0x1698819c
    .word 0x6487491a
    .word 0x3b5510db
    .word 0x2ca09355
    .word 0x0a60639a
    .word 0xbeffe246
    .word 0x9a560b26
    .word 0xe6afffec
    .word 0x5300ee74
    .word 0x311b6519
    .word 0xdcb26980
    .word 0xbd682d33
    .word 0xe9beba34
    .word 0xbe365642
    .word 0xade45b8f
    .word 0xc9037af1
    .word 0x8040976d
    .word 0x185465cf
    .word 0x7d1eba55
    .word 0xdc67c38e
    .word 0x595a2b58
    .word 0x8db450cc
    .word 0x4651e0d6
    .word 0x192956cb
    .word 0x3f0c81da
    .word 0x0b7ca918
    .word 0xbc66db30
    .word 0x1cb0adc8
    .word 0xdafd9b87
    .word 0x862253e3
    .word 0x1bf772d0
    .word 0x05a0aac1
    .word 0xc2825b0b
    .word 0x59782697
    .word 0xe0700370
    .word 0xf589d0d6
    .word 0xe69f1f9d
    .word 0x3951cc10
    .word 0xd2e4ea64
    .word 0xef0d80c6
    .word 0xeebdf785
    .word 0xa87df85a
    .word 0x08c91f22
    .word 0xf13a278a
    .word 0x173f4756
    .word 0x5e07fa84
    .word 0x0e7a430d
    .word 0x56d9e050
    .word 0x4aa28e29
    .word 0x792b196b
    .word 0xc2bcc281
    .word 0x342ee953
    .word 0x14de55c5
    .word 0x9b50bf87
    .word 0x91a9d828
    .word 0xfa147e78
    .word 0x8469cf03
    .word 0x9f4cced5
    .word 0xdbb80a8f
    .word 0x50520bf8
    .word 0x99370422
    .word 0x699f980e
    .word 0x2b39f497
    .word 0x8c4023e1
    .word 0x59d2dec5
    .word 0x04faedec
    .word 0xd6bbc6a8
    .word 0x95e5ba8e
    .word 0xcd5c260c
    .word 0x3db4c25a
    .word 0x4fdec31d
    .word 0x02d1f329
    .word 0xba9a9c73
    .word 0x6628efcb
    .word 0xd2190485
    .word 0xa506634c
    .word 0xa325e479
    .word 0xc05cb741
    .word 0x0b6401e2
    .word 0x2b8c6109
    .word 0xa818f980
    .word 0xe9f62df6
    .word 0x4b379437
    .word 0x1c382c91
    .word 0xfbd2b902
    .word 0x8f9b1f6e
    .word 0xacd283b3
    .word 0x0c33d394
    .word 0x1469e873
    .word 0xa5e660cb
    .word 0x9c3e3e1a
    .word 0x8151e10a
    .word 0x9cff3b53
    .word 0xdd8d61fa
    .word 0xfbe7943c
    .word 0xecaf07bf
    .word 0xc37f2211
    .word 0x4384842d
    .word 0x9a44fbf1
    .word 0xd13349e5
    .word 0x3029a88b
    .word 0x49d0dfea
    .word 0x6d477465
    .word 0xd32d8ae4
    .word 0xb7e398ef
    .word 0x7640e7ed
    .word 0x30163fc7
    .word 0xe1e3cb8d
    .word 0x91ab9b93
    .word 0x39711f06
    .word 0xcd086c94
    .word 0x1ddafc0b
    .word 0x0008d056
    .word 0xb55cef34
    .word 0x29649e34
    .word 0xa6a6c6e5
    .word 0x52672a9e
    .word 0x3200440c
    .word 0xe240dc3b
    .word 0x25babeec
    .word 0x70ecc64c
    .word 0xfa8b99fd
    .word 0x84c40a26
    .word 0xf48008a8
    .word 0x1c370049
    .word 0x8c36ff2c
    .word 0x36530da7
    .word 0xd867c89b
    .word 0x34d35db4
    .word 0xf3fd2d30
    .word 0xd90a89cb
    .word 0xc9753731
    .word 0x3fb2e5a0
    .word 0xd511bd55
    .word 0xb922b6b1
    .word 0xa1f7aa90
    .word 0x186c1b55
    .word 0x13fba540
    .word 0x018a1e81
    .word 0xf3cbe958
    .word 0x8950415e
    .word 0x67881be7
    .word 0x50669895
    .word 0x50ec7a73
    .word 0xe12ef6c4
    .word 0x8e1a8b44
    .word 0xf88c2ce5
    .word 0xdfe088af
    .word 0xbf3f5e37
    .word 0x96444972
    .word 0x5c418123
    .word 0x41029319
    .word 0xa627cc2a
    .word 0x5061781b
    .word 0x783856b6
    .word 0x9b4ca659
    .word 0x6d047548
    .word 0x9cfe8620
    .word 0x4de6b11d
    .word 0x44f7da6c
    .word 0x61dff09f
    .word 0xf98e33c9
    .word 0x1df33a80
    .word 0xa2cb4927
    .word 0xba39ae5b
    .word 0xd4918ff2
    .word 0x96b9463b
    .word 0xbbc99282
    .word 0xfce7eb32
    .word 0x73df032a
    .word 0x6ffc8f32
    .word 0x9e8b5323
    .word 0x9749bfbd
    .word 0xab1ec468
    .word 0x10326442
    .word 0x8687363e
    .word 0x5cc38f2d
    .word 0x105adab0
    .word 0xd84710d6
    .word 0xb01b51a2
    .word 0x1a2694ab
    .word 0xba6c3de7
    .word 0x760ec3da
    .word 0x673aed0a
    .word 0x6435b6e0
    .word 0xbd213068
    .word 0x4e9f42ce
    .word 0xdc50217c
    .word 0xf502ba24
    .word 0xff40fadd
    .word 0xaff3f27f
    .word 0xf066e835
    .word 0x28c759db
    .word 0xbfc437fe
    .word 0xc9ceef3b
    .word 0xca90aa48
    .word 0x78b42b08
    .word 0xeddd8aa8
    .word 0x694aea4f
    .word 0xa74ee5f1
    .word 0x646ab452
    .word 0x6329b0d1
    .word 0xaa1408ed
    .word 0x65e9c60a
    .word 0xf1001758
    .word 0xbc371cfe
    .word 0xf5b3e33d
    .word 0x585bf673
    .word 0xa46930b8
    .word 0xde5b20ec
    .word 0xd1a8e0e3
    .word 0xc9ba2a35
    .word 0x5b427135
    .word 0x7c059bee
    .word 0x5208534f
    .word 0xa81a6ab2
    .word 0x0d3f9331
    .word 0x3004f997
    .word 0xb5751c9f
    .word 0x1a626b24
    .word 0xdee92634
    .word 0x79b97dc8
    .word 0x47986872
    .word 0xd190e25f
    .word 0x1bc3d0e2
    .word 0x092eb565
    .word 0x4f49309b
    .word 0xb25d0923
    .word 0xded4f6a1
    .word 0x0495ac31
    .word 0x186aab20
    .word 0x529a5193
    .word 0xe2761ebb
    .word 0x5d85c6df
    .word 0xd3f86c79
    .word 0x301102c1
    .word 0xe0c1f591
    .word 0xc6a7d8f3
    .word 0x2bc33338
    .word 0x71aea31d
    .word 0xca3a605f
    .word 0xd18ced8b
    .word 0xe33c101d
    .word 0x7c550c6d
    .word 0xadb26983
    .word 0x9f9f4057
    .word 0x8210c3a9
    .word 0x9b40393b
    .word 0x496f2ba9
    .word 0xfe9d4cb4
    .word 0x42abdd1d
    .word 0xef92ad22
    .word 0xd278ed22
    .word 0xec71357d
    .word 0x0a661378
    .word 0x27bce278
    .word 0xc48e8652
    .word 0xb3a9bca2
    .word 0x6bea5955
    .word 0xfc8427fb
    .word 0x59ea86bc
    .word 0xad6c2141
    .word 0xf05fa735
    .word 0xb3171a1d
    .word 0x7b77aa70
    .word 0x3faa7234
    .word 0x0d034572
    .word 0x49209645
    .word 0xb079433d
    .word 0x35740566
    .word 0x1faee076
    .word 0x80a571c1
    .word 0xc9d5275a
    .word 0x72f8b7a0
    .word 0x12117214
    .word 0xe373e594
    .word 0xe4b84a7a
    .word 0x7c919f22
    .word 0x675bba8a
    .word 0xdb93f19d
    .word 0xe4a598f2
    .word 0x1ec535ca
    .word 0xe4e482dd
    .word 0xc68d5be5
    .word 0xa69fea41
    .word 0xf9cdcea0
    .word 0xf4a104dd
    .word 0x9a6ebd88
    .word 0x6caa3ceb
    .word 0x2faf2fe5
    .word 0x9865ec77
    .word 0x223390e4
    .word 0xd6bbfcf6
    .word 0x85b22a51
    .word 0xe53800b5
    .word 0x05b25935
    .word 0x671e925e
    .word 0x80fca2fa
    .word 0xdaf9c51d
    .word 0xa11d7b9e
    .word 0x2f0c67f6
    .word 0xd86978fc
    .word 0xd58e06e0
    .word 0x82814b7f
    .word 0xc2c202ce
    .word 0xd4cda2e6
    .word 0xb6ab6e65
    .word 0xe411dc42
    .word 0x27638ba8
    .word 0x2cecf9d3
    .word 0x4aefd9fc
    .word 0x79447534
    .word 0xdb22d5a3
    .word 0xa5f35d4b
    .word 0xa4c38904
    .word 0x95a0e657
    .word 0x276728e0
    .word 0x851848f2
    .word 0xe332a9b0
    .word 0x3c74399b
    .word 0xdce1afbc
    .word 0xbdbbecfb
    .word 0xd73ba7b9
    .word 0x349925f5
    .word 0x62963136
    .word 0x4f34d808
    .word 0x74296851
    .word 0x23a9bef5
    .word 0xd4d304c6
    .word 0xf937b353
    .word 0xc2927017
    .word 0x2963a482
    .word 0xc94c700a
    .word 0x671191ee
    .word 0x9154063d
    .word 0xcbb61a44
    .word 0x6f62b22a
    .word 0xf103578c
    .word 0xa5bf108c
    .word 0xa6cd984e
    .word 0xf20655eb
    .word 0xa33d85b6
    .word 0x9c065347
    .word 0xc8a9a652
    .word 0xb896472a
    .word 0x7a4f127e
    .word 0x597a67a3
    .word 0xb435ad69
    .word 0xa1af5ec2
    .word 0x782fec25
    .word 0x245973cc
    .word 0xcf397896
    .word 0xb0895c57
    .word 0x98101d62
    .word 0x0f4517da
    .word 0x9033423d
    .word 0xadda0bcb
    .word 0x2f93a419
    .word 0xb4a0e8ae
    .word 0xf95c6961
    .word 0x2142d556
    .word 0x347fac08
    .word 0xa999f45e
    .word 0x3c179ee5
    .word 0x630f6233
    .word 0x007cde33
    .word 0x47b135e5
    .word 0x4765c0b6
    .word 0x4cc88a5d
    .word 0x6d6f0cf2
    .word 0xa3e62a96
    .word 0x9cb97c62
    .word 0x8437ffc4
    .word 0xb6a01e79
    .word 0x333e9a46
    .word 0xbb5f058b
    .word 0xdeb9227d
    .word 0xae73d877
    .word 0xe70cea50
    .word 0x56c4c75e
    .word 0xa803d19d
    .word 0x61cc4347
    .word 0x2ee77cc0
    .word 0x3b77f65e
    .word 0x9e00c98b
    .word 0x6e413d24
    .word 0x45687037
    .word 0xbb482eb0
    .word 0xb3e473cc
    .word 0x645d0b05
    .word 0x915b5533
    .word 0x2d4bc9e5
    .word 0x075ec014
    .word 0xadf0b965
    .word 0x3c791f45
    .word 0xc9459f19
    .word 0x43fd24df
    .word 0x2ba1bf02
    .word 0x540f4ff4
    .word 0x69b8ed06
    .word 0xc7c6b40c
    .word 0x6129f756
    .word 0x8b1cef72
    .word 0xf34aa093
    .word 0x41f42589
    .word 0xef69b60c
    .word 0x8bc9b821
    .word 0x538b8694
    .word 0x524649bb
    .word 0x7eaf9cbd
    .word 0xa0e16631
    .word 0xc33fe69b
    .word 0x31c5bcb1
    .word 0x41a912e9
    .word 0x0bd7ccd6
    .word 0x8295f6ca
    .word 0x00de3966
    .word 0xa38d9120
    .word 0xbce7c929
    .word 0xd7e9d206
    .word 0x9972725c
    .word 0xefc7e356
    .word 0xbb34deb3
    .word 0x10a49889
    .word 0xaeaf1ed5
    .word 0x94e90eba
    .word 0x4c57b74b
    .word 0xb0872efd
    .word 0x084453d0
    .word 0x655b39a5
    .word 0xd94ec7d7
    .word 0x081e72c6
    .word 0x2d0dc3ac
    .word 0x8f0b46b2
    .word 0x540c8bd0
    .word 0xd3d1ed00
    .word 0x80dafeff
    .word 0x8271bdbf
    .word 0x2750d92c
    .word 0x5627a929
    .word 0x34e1dcce
    .word 0x3b6170f4
    .word 0xe581dccc
    .word 0xf7ffa014
    .word 0x44e07cd3
    .word 0x944236e7
    .word 0xbc93ed03
    .word 0x6f33e29a
    .word 0x35eb8afd
    .word 0x260d5f6a
    .word 0xef6b1009
    .word 0x161d52bb
    .word 0x92170096
    .word 0xeff08bad
    .word 0x8f17a917
    .word 0x5d0bffd9
    .word 0xbc3b1a22
    .word 0x39362304
    .word 0x2d5fee1f
    .word 0xe422da70
    .word 0xebe21d21
    .word 0x9293c4d1
    .word 0x77843448
    .word 0x9d24134b
    .word 0xf4695e8f
    .word 0x1dd658a9
    .word 0x9b008e16
    .word 0xfdce89ba
    .word 0xd87fa438
    .word 0xe996b8cf
    .word 0xaba6a636
    .word 0x388c3fef
    .word 0xb85189d5
    .word 0x3fe7db72
    .word 0xda00d0db
    .word 0x99da7ce2
    .word 0xf9d3dddf
    .word 0x32b267de
    .word 0x0c822106
    .word 0xf078c2e0
    .word 0xcee7d131
    .word 0xb3955ba5
    .word 0x7b674875
    .word 0x36ea6915
    .word 0x888cd0f6
    .word 0x6799d43b
    .word 0x77a6cb4c
    .word 0x1f496ece
    .word 0x2863afdc
    .word 0xa9a4cd02
    .word 0xa2acdeca
    .word 0x834f187f
    .word 0xb7f7103d
    .word 0x17e3550e
    .word 0xd72a8dd4
    .word 0x417197f3
    .word 0x7eb4abe5
    .word 0x7af6fac9
    .word 0xee475e55
    .word 0x230c4edc
    .word 0x7b6b879c
    .word 0x7e2fae8f
    .word 0xadd582db
    .word 0x2ae27c38
    .word 0x141290a9
    .word 0xa66eecfb
    .word 0xb7cfc0c4
    .word 0x82bce33f
    .word 0xad4d069b
    .word 0x7a642687
    .word 0x2f2c3b73
    .word 0x3bbb0601
    .word 0x73392cad
    .word 0xbd8718c7
    .word 0xdb0672ca
    .word 0x4a40c09b
    .word 0xffb2cee6
    .word 0x4f4d8a9a
    .word 0x655b369a
    .word 0x342124e8
    .word 0xf4715869
    .word 0xcdb42f7f
    .word 0xc57805aa
    .word 0xc4544af0
    .word 0x32ea3ece
    .word 0x073a66f4
    .word 0x3e38659a
    .word 0x11c2228e
    .word 0x6bd4808f
    .word 0x6dec6891
    .word 0xf2f3797e
    .word 0x7f9517e4
    .word 0x464fded3
    .word 0xad01bc01
    .word 0x96f8ddf6
    .word 0x6d2765a3
    .word 0xa12327b2
    .word 0xa65a0f61
    .word 0x66030d82
    .word 0x7edae34b
    .word 0x60188422
    .word 0xe512f330
    .word 0x1df75d85
    .word 0xd1fb8ab5
    .word 0xed74c06e
    .word 0xec1c16bb
    .word 0xb73593d0
    .word 0xaefdce70
    .word 0x678e48a7
    .word 0x06571ad7
    .word 0x0b50ec84
    .word 0xf29f84a5
    .word 0x4fac20e9
    .word 0x0923af40
    .word 0x17b8a300
    .word 0x4c3fb26f
    .word 0x58406559
    .word 0xd6f6ab68
    .word 0x2ff6702c
    .word 0x0b6f4d88
    .word 0x6c4ac780
    .word 0x5edf1cef
    .word 0x7a996163
    .word 0xd69f4c53
    .word 0x782eb47c
    .word 0x9800a7bb
    .word 0x7ea6a9d2
    .word 0xdd81849a
    .word 0x9d1ad4d6
    .word 0xe3e9a490
    .word 0xa874f1f8
    .word 0x948c3a10
    .word 0x866aa3f0
    .word 0x15559b40
    .word 0xfc2add38
    .word 0x7089bc02
    .word 0xb81b1393
    .word 0x553ba505
    .word 0x55e95b19
    .word 0xb2199aca
    .word 0xb5d0bafa
    .word 0x771c012e
    .word 0x4dcd4576
    .word 0xfeda7263
    .word 0x79a9c0c2
    .word 0xce0439b1
    .word 0xde14d39d
    .word 0x39a3212c
    .word 0xe755c209
    .word 0xac922dd9
    .word 0xfeeba1bb
    .word 0x28921725
    .word 0x9ed945a8
    .word 0x2579833e
    .word 0x4410216c
    .word 0x1ebc9753
    .word 0x3dc9f844
    .word 0x319b5a3c
    .word 0x952d9961
    .word 0x5d6a10bb
    .word 0xc7a381b0
    .word 0x72ad58ae
    .word 0x2c167252
    .word 0x20735da1
    .word 0xd605cc73
    .word 0xca61fbfd
    .word 0x34ea00c5
    .word 0x5324afbf
    .word 0x08a28a38
    .word 0xb5efb55d
    .word 0xf3049f01
    .word 0xc856e06f
    .word 0x6cb940a4
    .word 0xb9a3f364
    .word 0x091f7113
    .word 0xe98f53fe
    .word 0x9f1118a0
    .word 0x565003f3
    .word 0xc6a0440e
    .word 0x09649a4d
    .word 0x12332dab
    .word 0x9ad1a68c
    .word 0x9c3fecb5
    .word 0x4fd56948
    .word 0x31b1e812
    .word 0x9ae6f2c9
    .word 0xf5d7bb63
    .word 0xd03114b9
    .word 0x85c874d2
    .word 0x6ed18c18
    .word 0x616383b3
    .word 0x277b6c70
    .word 0xc3237c71
    .word 0x213394c9
