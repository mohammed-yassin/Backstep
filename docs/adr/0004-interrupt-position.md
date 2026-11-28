# ADR 0004: Recording where each interrupt arrived

**Status:** Accepted (M1). Replay of these keys: [ADR 0009](0009-replaying-interrupts.md) (M4)

## Context

Replay must deliver every interrupt before exactly the same instruction
as on the real chip. Cortex-M3/M4 have no retired-instruction counter, so
the position cannot be recorded as "instruction number N". It has to be
recognisable from the machine state.

## Decision

At every interrupt entry a naked stub (`BS_IRQ_STUB`,
`recorder/src/irq_record.c`) records the **interruption key**:

| Field | Why |
|-------|-----|
| PC where the interrupted code resumes | the instruction |
| r0-r12, SP, LR, xPSR (alignment-pad bit removed) | the full register state |
| Peripheral reads made so far (MPU trap count) | separates iterations of polling loops |
| FNV-1a hash of 128 bytes of live stack above SP | separates iterations whose loop state is kept in stack slots |

Replay injects the interrupt at the first instruction, after the previous
event, whose emulated state equals the key.

The stub masks interrupts at its first instruction. A higher-priority
interrupt can still arrive before that instruction; in that case the inner
event resumes at the stub's first instruction, and the outer event takes
the read count of its true interruption point from it (unit-tested in
`recorder/tests/test_irq_record.c`, because QEMU almost never produces it).

## Evidence

`scripts/irq-position-test.sh` (in CI) runs the test firmware 7 times
(5 seeds at -O2, 2 at -O0) with two interrupt sources at random intervals.
SysTick at priority 0x40 nests inside timer 0 at 0x80. The workloads are
checksums, sorting, recursion, peripheral polling, spinning on a RAM flag,
memory copies and indirect calls. QEMU logs the register state before every
instruction (`-icount shift=0`, one instruction per translation block), and
`scripts/irq-position-check.py` checks every recorded event against it:

| Result (14,035 interrupts, of which 7,024 tail-chained and 7,114 nested) | Count |
|---|---|
| Recorded context or read count differs from the truth | **0** |
| Key unique since the previous interrupt | 13,708 |
| Earlier state with the same key, no stores in between (identical machine state, harmless) | 229 |
| Earlier state with the same registers and read count, but live stack slots changed. The **stack hash rules it out** | 98 |
| Unresolved by the key | 0 |

The checker proves it can fail: a deliberately corrupted event is
detected. Writing the checker also exposed and fixed two QEMU logging
artefacts: an instruction logged before an interrupt pre-empts it, and an
instruction logged twice when `-icount` restarts a block. Both are handled
explicitly and documented in the script.

### What we learned

1. **PC + registers is not enough.** At -O2, GCC kept a recursion's loop
   state in stack slots while every register repeated. Without the stack
   hash, 98 of these interrupts would have been injected at the wrong
   iteration.
2. **There is a residual case the key cannot decide.** In an earlier
   configuration of the same test, 1 interrupt in 2,007 arrived in a
   superloop whose state lives only in global variables while every
   register and the stack repeat exactly. No cheap on-device key separates
   such iterations on Cortex-M4.

## Consequences

- The replay engine must support **lookahead on ambiguous keys**: done
  in M5, see ADR 0009.
  When a key matches, it records a checkpoint and continues. If the next
  recorded data disagrees (next interrupt key never reached, a peripheral
  read at a different address, or a state checksum mismatch), it rewinds
  and tries the next match. Correctness then rests on the recorder's state
  checksums, which must cover registers and live RAM. That makes the
  M3/M4 acceptance tests for divergence detection essential.
- Cost per interrupt: saving the context plus a 32-word hash, roughly
  150-250 instructions. Interrupt rates above a few hundred kHz on a
  100 MHz part are out of scope while recording.
- Only the main stack (MSP) is supported by the prototype; PSP-based RTOS
  threads are added with FreeRTOS support (M8).
- On cores with DWT, CYCCNT can be recorded as a hint to rank ambiguous
  matches; it is not needed for correctness.
