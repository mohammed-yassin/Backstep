# Changelog

All notable changes to Backstep. Versions follow [Semantic
Versioning](https://semver.org/); before 1.0 the recording format and the
recorder API may still change between minor versions (a recording names
its format version, and the tools say when they cannot read it).

## [Unreleased]

### Added

- `backstep why <variable>`: the last write to a variable before the end
  of the recording (or `--at` a position): the instruction, source line,
  function and context (main code or which interrupt handler), with the
  value before and after. It also lists every earlier write, flags
  writes through the stack pointer (a stack overflow into a global), and
  finds lost updates (a race between an interrupt and the code it
  interrupted). Variables are named as in C (`g_rx.limit`, `buf[3]`),
  using the ELF's DWARF information ([guide](docs/QUERIES.md),
  [ADR 0011](docs/adr/0011-queries.md)).
- `backstep timeline`: every exception entry and return in a recording.
  For a fault, it shows the causes the core recorded and the instruction
  executed before.
- `--json` for both.
- Example bugs: `demo_stack.c` (a task stack overflow) and `demo_race.c`
  (a lost update), next to `demo_overrun.c`.

## [0.1.0] - 2026-11-28

The first release: record firmware on a Cortex-M4, replay it exactly on
the PC, and debug the replay with GDB, forwards and backwards.

### Recorder (C99, `recorder/`)

- Peripheral reads captured with an MPU trap (any build, no compiler
  changes), or with a Clang pass ([ADR 0003](docs/adr/0003-peripheral-read-capture.md)).
- Interrupt positions recorded by a stub per interrupt: resume PC,
  registers, peripheral-read count and a stack hash
  ([ADR 0004](docs/adr/0004-interrupt-position.md)).
- Starting-state snapshot of registers, RAM and the System Control Space
  ([ADR 0005](docs/adr/0005-starting-state.md)).
- `bs_record_begin()` / `bs_record_end()` / `bs_record_write()`: a
  recording file with checksums on every part, format version 2
  ([ADR 0008](docs/adr/0008-recording-format.md)).
- A fault while recording ends the recording (a crash dump).

### Host tools (Rust, `host/`)

- A Cortex-M4 emulator for the whole Thumb/Thumb-2 integer and DSP
  instruction set and the exception model, checked against QEMU over
  1,000,000 instructions on every change
  ([ADR 0006](docs/adr/0006-emulator.md), [ADR 0007](docs/adr/0007-exceptions.md)).
- `backstep replay`: re-runs a recording and checks every peripheral
  load, every interrupt position and the end state; any difference is
  reported, never ignored. Lookahead resolves interrupt positions that
  loop iterations share ([ADR 0009](docs/adr/0009-replaying-interrupts.md)).
- `backstep gdb`: a GDB server over the replay. Step, continue,
  breakpoints, watchpoints, and reverse-step, reverse-continue,
  reverse-next, reverse-finish ([ADR 0010](docs/adr/0010-reverse-debugging.md),
  [guide](docs/GDB.md), [VS Code](docs/VSCODE.md)).
- `backstep inspect`, `backstep crc32`, `backstep info`.

### Targets

- QEMU `mps2-an386` (Cortex-M4): the reference target for the automated
  tests.
- ST NUCLEO-F411RE: test images and bench scripts for every milestone;
  the runs on the real board are queued in
  [HARDWARE_VERIFICATION.md](docs/HARDWARE_VERIFICATION.md).

### Known limits

- No floating-point unit in the emulator yet (M7): firmware must not use
  the FPU while recording.
- Main stack only for interrupt keys; RTOS threads come in M8.
- Peripheral reads at priority -1/-2 (HardFault, NMI) and Private
  Peripheral Bus reads (SysTick, DWT) are not recorded; replay says so.
