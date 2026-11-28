/*
 * Backstep interrupt-position recording (EXPERIMENTAL, milestone M1).
 *
 * Replay must deliver each interrupt before exactly the same instruction
 * as on the real chip. The recorder therefore logs, at every interrupt
 * entry, the complete interrupted CPU context: the PC where execution
 * will resume, all core registers, xPSR, and the number of peripheral
 * reads made so far. Replay injects the interrupt at the first point
 * where its emulated state equals that key. See
 * docs/adr/0004-interrupt-position.md for why this is sufficient.
 *
 * Usage: route an exception vector through a stub generated with
 * BS_IRQ_STUB(stub_name, handler_name); the stub records the context and
 * tail-branches to your handler.
 *
 * SPDX-License-Identifier: MIT
 */
#ifndef BACKSTEP_IRQ_H
#define BACKSTEP_IRQ_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

struct bs_irq_event {
    uint32_t exception;   /* IPSR of the exception being entered */
    uint32_t r[13];       /* r0-r12 of the interrupted context */
    uint32_t sp;          /* SP of the interrupted context, before stacking */
    uint32_t lr;          /* LR of the interrupted context */
    uint32_t pc;          /* PC where the interrupted context resumes */
    uint32_t xpsr;        /* xPSR of the interrupted context (pad bit cleared) */
    uint32_t mmio_index;  /* peripheral reads made before this point */
    uint32_t stack_hash;  /* bs_stack_hash() of the live stack top */
};

/* Bytes of live stack (from the interrupted SP upwards) folded into
 * stack_hash. Optimised code may keep loop state in stack slots while
 * every register is unchanged between iterations; the hash tells such
 * iterations apart (docs/adr/0004-interrupt-position.md). */
#define BS_STACK_HASH_BYTES 128u

/* Dead stack below the stub's SP that the stub zeroes after
 * bs_irq_record() returns: at least the stack bs_irq_record() and
 * bs_stack_hash() use (80 bytes at -O0 with GCC; checked by
 * scripts/check-stack-usage.py). The stub stores 8 x 16 bytes. */
#define BS_IRQ_STUB_SCRATCH_BYTES 128u

/* FNV-1a over 32-bit words; the replay engine computes the same. */
uint32_t bs_stack_hash(const uint32_t *words, uint32_t count);

/* Start recording into log[0..capacity-1] (log may be 0: events are then
 * only counted, as are events beyond the capacity). mmio_counter points at the
 * running count of captured peripheral reads (e.g. the MPU trap's);
 * stack_top is the initial main stack pointer (end of the stack). */
void bs_irq_record_start(struct bs_irq_event *log, uint32_t capacity,
                         const volatile uint32_t *mmio_counter,
                         const void *stack_top);
uint32_t bs_irq_record_count(void);

/* Stop recording (later interrupts are neither logged nor counted). */
void bs_irq_record_stop(void);

/* Called by the stubs; not for direct use. */
void bs_irq_record(const uint32_t *callee_r4_r11, const uint32_t *frame,
                   uint32_t ipsr, uint32_t stub_address);

/*
 * Naked entry stub: masks interrupts at its very first instruction, saves
 * r4-r11 (the hardware already stacked r0-r3, r12, lr, pc, xpsr), records
 * the context, restores r4-r11 and lr, unmasks, and tail-branches to the
 * real handler with the original EXC_RETURN in lr. The handler starts with
 * r0-r3, r12 and the APSR flags zero (they are scratch at a handler's
 * entry), and the 256 bytes of dead stack bs_irq_record() used are zeroed,
 * so neither registers nor memory depend on what the recorder computed:
 * replay, which runs the same stub without recording, sees the same state.
 *
 * A higher-priority interrupt can still arrive before that first
 * instruction; bs_irq_record() detects this case (the inner event resumes
 * at this stub's first instruction) and gives the outer event the
 * peripheral-read count of its true interruption point.
 * Only the main stack (MSP) is supported by this prototype.
 */
#define BS_IRQ_STUB(stub, handler)                                           \
    __attribute__((naked)) void stub(void)                                   \
    {                                                                        \
        __asm__ volatile("cpsid i\n\t"                                       \
                         "push {r4-r11, lr}\n\t"                             \
                         "sub sp, #4\n\t"                                    \
                         "add r0, sp, #4\n\t"                                \
                         "add r1, sp, #40\n\t"                               \
                         "mrs r2, ipsr\n\t"                                  \
                         "movw r3, #:lower16:" #stub "\n\t"                  \
                         "movt r3, #:upper16:" #stub "\n\t"                  \
                         "bl bs_irq_record\n\t"                              \
                         /* zero BS_IRQ_STUB_SCRATCH_BYTES below SP, what */ \
                         /* bs_irq_record used, and the scratch registers */ \
                         "mov r3, sp\n\t"                                    \
                         "mov.w r0, #0\n\t"                                  \
                         "mov.w r1, #0\n\t"                                  \
                         "mov.w r2, #0\n\t"                                  \
                         "mov.w r12, #0\n\t"                                 \
                         "stmdb r3!, {r0-r2, r12}\n\t"                       \
                         "stmdb r3!, {r0-r2, r12}\n\t"                       \
                         "stmdb r3!, {r0-r2, r12}\n\t"                       \
                         "stmdb r3!, {r0-r2, r12}\n\t"                       \
                         "stmdb r3!, {r0-r2, r12}\n\t"                       \
                         "stmdb r3!, {r0-r2, r12}\n\t"                       \
                         "stmdb r3!, {r0-r2, r12}\n\t"                       \
                         "stmdb r3!, {r0-r2, r12}\n\t"                       \
                         "mov.w r3, #0\n\t"                                  \
                         "msr apsr_nzcvqg, r0\n\t"                           \
                         "add sp, #4\n\t"                                    \
                         "pop {r4-r11, lr}\n\t"                              \
                         "cpsie i\n\t"                                       \
                         "b " #handler "\n\t");                              \
    }

#ifdef __cplusplus
}
#endif

#endif /* BACKSTEP_IRQ_H */
