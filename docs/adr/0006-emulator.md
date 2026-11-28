# ADR 0006: The replay emulator and how it is verified

**Status:** Accepted (M2, parts 1 and 2; exceptions in [ADR 0007](0007-exceptions.md); floating point, added in M7, in [ADR 0012](0012-floating-point.md))

## Context

Replay runs the user's unmodified firmware binary on a Cortex-M4 emulator
on the PC (ADR 0001: written from scratch, MIT licensed). An emulator bug
would silently produce a false history, so correctness is verified
instruction by instruction, not just by "programs finish".

## Decision

**Crate `backstep-emu`** (`host/crates/backstep-emu`):

- `Cpu`: r0-r15, flags (N Z C V Q, GE), ITSTATE, IPSR, PRIMASK,
  BASEPRI, FAULTMASK, CONTROL, the banked stack pointer, the local
  exclusive monitor and memory. `step()` executes exactly one instruction
  or returns a `Stop` with every register unchanged. Stops cover
  breakpoints, SVC, undefined/UNPREDICTABLE encodings, unimplemented
  encodings (floating point until M7), bus faults, unaligned accesses by
  instructions that require alignment (a UsageFault on the chip), invalid
  interworking targets and exception returns. It never guesses.
- 16-bit instructions are in `cpu.rs`, 32-bit Thumb-2 in `cpu/thumb32.rs`,
  one function per decode table of the manual (section A5.3).
- `Memory`: RAM/flash regions; any other address goes to a `Bus` trait.
  During replay the bus answers peripheral loads from the recording.
- `elf`: a dependency-free ELF32 loader (load segments, symbols).
- Semantics follow the ARMv7-M Architecture Reference Manual pseudocode
  (`AddWithCarry`, `Shift_C`, `ITAdvance`, `BXWritePC`...), cited in code.

**Verification: differential testing against QEMU**
(`scripts/emu-difftest.py`):

1. Generate random programs (seeded), covering every 16-bit Thumb
   instruction class, including IT blocks, forward branches, PC/SP
   operands, literal pools and all load/store forms. The initial registers,
   index, stack pointer and flags are random too.
2. Assemble them with the GNU assembler (`.n` forces the 16-bit
   encodings).
3. Run each on QEMU `mps2-an386`, which logs r0-r15 and xPSR before every
   instruction, and on `backstep-emu-trace`, which prints the same format.
4. Require identical states at every step, and that every instruction
   class was actually executed.

QEMU's reset values of the flags are architecturally UNKNOWN, so states
are compared without flags until the program has set them.

Part 2 extends the generator to every 32-bit encoding: data processing
with modified immediates, shifted registers and SP operands; ADDW/SUBW,
ADR, MOVW/MOVT; saturation, bitfields, extends, the 36 parallel
add/subtract instructions, SEL, saturating arithmetic, bit reversal, CLZ;
all multiplies, DSP multiplies and divides; every load/store addressing
mode (12-bit, negative, pre/post-indexed, register, literal, unprivileged,
hints), LDRD/STRD, LDM/STM/PUSH/POP, exclusives, TBB/TBH, forward and
backward B.W/B<cond>.W/BL, branches ending IT blocks, loads into PC,
MSR/MRS of every special register, running on PSP, barriers and hints.
Values are chosen so the rare paths happen: sequences that clear the
sticky Q flag right before an instruction that may saturate, with
operands near the limits; division by zero; INT_MIN / -1. Coverage is
counted per encoding width (`ldr` and `ldr.w` are different classes) and
an instruction the classifier does not know fails the run.

**Decisions on IMPLEMENTATION DEFINED behaviour**

- *Exclusive monitor.* A STREX succeeds if the monitor is in the
  Exclusive state and, when `exclusive_tags_address` is set (the
  default), the address matches the LDREX. The access size is not
  compared and a plain store does not clear the monitor; QEMU behaves the
  same way (measured by `exclusive_hw.elf` on QEMU's STM32F405). Some
  Cortex-M local monitors compare no address at all, so the STM32F411 is
  measured on the board (`scripts/hil/m2_nucleo_f411re.py`) and the
  replay default for STM32F4 recordings follows that measurement. Part 3
  clears the monitor on exception entry and return, as QEMU does.
- *BASEPRI* keeps only the implemented priority bits (`priority_mask`):
  all 8 in QEMU `mps2-an386` (measured by the differential test), 4 on
  the STM32F4 (`0xF0`), set by the replay target description.
- *Division by zero* gives 0 (CCR.DIV_0_TRP is 0 after reset; the trap
  becomes part of the fault model in part 3).
- *FAULTMASK* can be cleared at any time and set below priority -1 (not
  in NMI or HardFault); writes from unprivileged code are ignored, as are
  all privileged special-register writes and CPS.

## Evidence (part 1: 16-bit Thumb)

- 191,317 consecutive instructions over 100 random programs (4 seeds)
  were identical to QEMU, with all 50 instruction classes executed.
  CI runs 40 programs (about 60,000 instructions) on every change.
- The differential test found one real bug while it was being built:
  REV, REV16 and REVSH were decoded from the wrong opcode bits.
- Mutation testing: deliberately broken SBC carry, SXTB sign extension,
  IT-state advance and LDR-literal alignment are each caught.
- Unit tests (all platforms) cover what QEMU cannot show: stop
  conditions, UNPREDICTABLE encodings (including instructions illegal in
  IT blocks whatever their condition), and rollback of state on faults.

## Evidence (part 2: 32-bit Thumb-2)

- 278,431 consecutive instructions over 200 random programs (4 seeds)
  were identical to QEMU, with all 281 instruction classes (16-bit and
  32-bit counted separately) executed. Together with the runs made while
  building, over 690,000 Thumb-2-mix instructions have matched. CI runs 40
  programs (about 55,000 instructions) on every change.
- The differential test found a real decoder bug while it was being
  built: ADDW/SUBW rejected LR as the base register.
- It also exposed a weakness of the test itself: the Q flag is sticky, so
  once any instruction set it, a missing Q update elsewhere was invisible.
  The generator now clears Q before every candidate instruction and uses
  operands near the saturation limits.
- Mutation testing: 30 deliberately introduced bugs are each caught,
  among them the modified-immediate carry and replication patterns, Q
  from SMLAD, SMLA<x><y>, SMLAW, SSAT and QDADD, GE thresholds,
  ASX/SAX lane order, LDRD/STRD word order and literal alignment, LDM
  writeback, STREX status, TBB/TBH scaling, extend rotation, sign
  extension of LDRSB, B<cond>.W sign extension, the BL/B.W I1 bit,
  BFI positioning, SMMLAR rounding, UMAAL, division by zero, MRS of GE,
  BASEPRI_MAX, CONTROL.SPSEL banking and shifted SP operands.
- Unit tests cover the rest: UNPREDICTABLE encodings (one-register LDM,
  LDRD into one register, BFI with msb below lsb, MOVW to SP, STREX status
  equal to data, UDF.W, BLX immediate, loads into PC not last in an IT
  block, B<cond>.W inside IT), floating point reported as unimplemented,
  alignment faults, branch offsets up to 16 MiB that the 4 MiB test image
  cannot reach, unprivileged writes, SP banking in thread and handler
  mode, FAULTMASK in HardFault, the exclusive monitor's failure cases and
  rollback after a fault in the middle of an LDM.

## Consequences

- Part 3 of M2 extended the same harness to exceptions
  ([ADR 0007](0007-exceptions.md)), compares memory at the end of every
  program, and runs 1,000,000 instructions in CI. BKPT is no longer a
  compared class: the programs' semihosting calls are now in the
  uncompared epilogue (unit tests cover BKPT).
- The part 1 and 2 generators never emitted 16-bit BX or BLX (register);
  this was found while adding exceptions, and both are generated and
  required classes now.
