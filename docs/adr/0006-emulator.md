# ADR 0006: The replay emulator and how it is verified

**Status:** Accepted (M2, part 1)

## Context

Replay runs the user's unmodified firmware binary on a Cortex-M4 emulator
on the PC (ADR 0001: written from scratch, MIT licensed). An emulator bug
would silently produce a false history, so correctness is verified
instruction by instruction, not just by "programs finish".

## Decision

**Crate `backstep-emu`** (`host/crates/backstep-emu`):

- `Cpu`: r0-r15, flags, ITSTATE, IPSR, PRIMASK and memory. `step()`
  executes exactly one instruction or returns a `Stop` with the state
  unchanged. Stops cover breakpoints, SVC, undefined/UNPREDICTABLE
  encodings, unimplemented encodings, bus faults, invalid interworking
  targets and exception returns. It never guesses.
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

## Consequences

- Parts 2 and 3 of M2 extend the same harness to 32-bit Thumb-2 and to
  exceptions; the generator grows with the decoder.
- The full M2 acceptance target (1,000,000 random instructions, zero
  mismatches) runs as a long job in addition to the per-change CI run.
