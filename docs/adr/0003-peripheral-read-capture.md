# ADR 0003: Capturing peripheral reads

**Status:** Accepted (M1). Cycle cost on real hardware pending
([HARDWARE_VERIFICATION.md](../HARDWARE_VERIFICATION.md)).

## Context

Replay needs the value of every load from a peripheral register (UART
status, timer counters, ADC results...), in order. Missing even one read
makes the replay diverge. Two ways to capture them were prototyped.

### Option A: MPU trap (no recompilation)

`recorder/src/mpu_trap.c`. The MPU makes 0x40000000-0x5FFFFFFF
inaccessible. Each peripheral load or store faults; the handler decodes the
instruction (`recorder/src/thumb_ldst.c`), performs the access itself with
the MPU briefly off, logs loads, applies writeback and IT-state advance,
and resumes after the instruction. Accesses from interrupts at priority 0
escalate to HardFault and are handled there. Anything it cannot emulate
exactly (LDM/STM, LDRD/STRD, exclusives, decoder/MMFAR disagreement, a
genuine fault) stops the core through `bs_mmio_trap_fatal()`; it never
guesses.

### Option B: Clang pass (compile-time instrumentation)

`tools/clang-mmio-pass` rewrites every volatile 8/16/32-bit load into a
call to `__bs_mmio_load*()` (`recorder/src/cc_hooks.c`), which loads and
logs when the address is in the captured range.

## Evidence (QEMU, `-icount shift=0`, exact instruction counts)

| | MPU trap | Clang pass |
|---|---|---|
| Same workload: 224 peripheral reads + 320 RAM-window reads of every size and sign | 544/544 captured | 544/544 captured |
| Logs (address, value, size, order) | byte-identical to the pass | byte-identical to the trap |
| Extra instructions per captured read | ~217 | ~27 (≈10 even with capture off) |
| Reads in assembly or precompiled libraries | captured | **missed** |
| Works with GCC, IAR, Keil, unmodified builds | yes | Clang only |
| Reads from interrupts at any priority | yes (tested at 0x80 and 0) | yes |

Correctness tests (all in CI):
- 1006 load/store encodings produced by the GNU assembler decode to the
  expected fields; 2.6 M random encodings decode consistently; ITAdvance
  checked for all 256 IT states.
- On QEMU, 14 load forms (16/32-bit, register/immediate, pre/post-index,
  negative offsets, sign extension, LR/R12/R8/R9 destinations) return the
  same value trapped as untrapped, on RAM and on a real device model, and
  each is logged exactly once.
- Conditional loads in IT blocks: taken loads trap and the remaining IT
  instructions keep their conditions; skipped loads cause no access.
- Stores of every size reach the device; writeback is correct.
- A genuine MemManage fault after many trapped loads is reported, not
  emulated (requires clearing the fault status bits; a mutant that skips
  this is caught).
- Mutation testing: deliberately broken writeback, sign extension, IT
  advance and status clearing are all detected by the test suite.

## Decision

1. **The MPU trap is the default capture mechanism.** Completeness is
   what replay needs, and only the trap sees every read, whatever compiler
   built it, including vendor binary libraries and inline assembly.
2. **The Clang pass is kept as an optional fast mode** for projects that
   build everything with Clang and need lower overhead. It shares the
   event format, so the host side does not care which one produced a log.
3. The trapped region is configurable (`bs_mmio_trap_enable_region`) for
   external memory-mapped devices and for tests.

## Consequences and limits

- Each trapped read costs a few hundred cycles (exact figure to be
  measured on NUCLEO-F411RE). Acceptable for debugging; field recording
  (M9) will need to keep the log compact.
- The application must not use the MPU while recording (a future version
  can share regions).
- Peripheral accesses inside HardFault and NMI handlers are not captured
  (the MPU is bypassed there). Replay treats the first instruction of
  those handlers as an end of recording.
- LDM/STM/LDRD/STRD to peripherals stop the core with a clear reason.
  None of the tested code generates them for volatile register access;
  if real firmware does, support is added with tests first.
