# Testing

Backstep's value depends on replay being *exact*, so testing is part of
the design, not an afterthought. This document explains what is tested,
how, and how to run it.

## Rules

1. Every change comes with tests, including failure cases.
2. A pull request is merged only when `scripts/ci.sh` passes locally **and**
   GitHub Actions CI is green.
3. Warnings are errors everywhere: C (`-Wall -Wextra -Werror` plus extra
   flags), Rust (`clippy -D warnings`, docs with `-D warnings`).
4. Tests must be able to fail. New checks are tried against a broken input
   at least once to confirm they catch it (mutation testing: introduce a
   bug on purpose, confirm the suite reports it).
5. Behaviour that only real hardware can confirm is listed in
   [HARDWARE_VERIFICATION.md](HARDWARE_VERIFICATION.md) with a script to
   check it.

## Test layers

| Layer | What it proves | Where |
|-------|----------------|-------|
| Rust unit and integration tests | Host logic and the CLI behave as specified | `host/crates/*/src`, `host/crates/*/tests` |
| Recorder unit tests (ASan + UBSan) | Recorder logic is correct and memory-safe | `recorder/tests/` |
| Cross-language checks | C, Rust and Python `zlib` compute identical CRC-32 values | `recorder/tests/`, `crc32.rs`, `scripts/ci.sh` |
| QEMU self-tests | Firmware startup, interrupts and recorder code run correctly on a Cortex-M4 | `firmware/targets/qemu-mps2-an386/`, `scripts/qemu-run.sh` |
| QEMU board smoke tests | Nucleo firmware programs the right registers, and the Nucleo trap and exclusive-monitor tests run end to end, before reaching hardware | `scripts/qemu-nucleo-smoke.sh`, `scripts/qemu-nucleo-trap-smoke.sh`, `scripts/qemu-nucleo-excl-smoke.sh` |
| Decoder vectors | The load/store decoder agrees with the GNU assembler on 1006 encodings; 2.6 M random encodings decode consistently | `recorder/tests/test_thumb_ldst.c`, `gen_ldst_vectors.py` |
| MPU trap tests (QEMU) | Every load/store form, IT blocks, interrupts at both priorities, genuine-fault detection | `firmware/targets/qemu-mps2-an386/mmio_trap_test.c` |
| Capture comparison | MPU trap and Clang pass capture byte-identical logs of the same workload | `scripts/capture-compare.sh` |
| Interrupt positions | 14,000 recorded interrupt contexts match QEMU's per-instruction trace; the key is unique or provably safe | `scripts/irq-position-test.sh`, `scripts/irq-position-check.py` |
| Snapshot restore | A snapshot restored into a fresh QEMU continues identically; leaving out any part changes the result | `scripts/snapshot-restore-test.sh`, `scripts/snapshot_tool.py` |
| Interrupt recorder logic | Context fields, alignment padding, stack hash, preemption before the stub's first instruction, counting without a log and after stop | `recorder/tests/test_irq_record.c` |
| Emulator differential test | 1,000,000 instructions of random Thumb and Thumb-2 programs with exceptions: every register and xPSR (flags, Q, GE, IT, IPSR) identical to QEMU before every instruction, data and stack RAM identical at the end; all 287 instruction classes and 22 kinds of exception event (including lockup) happen | `scripts/emu-difftest.py` |
| Floating-point differential test (M7) | 1,000,000 floating-point instructions (FPv4-SP) in random programs that also run integer code and take exceptions: S0-S31 and FPSCR identical to QEMU before every instruction, with every core register, and RAM at the end; operands biased to edge values (NaNs, infinities, denormals, limits); all 75 floating-point instruction classes (arithmetic, conversions, compares, transfers, loads and stores, in IT blocks), NOCP, UNDEFINED and alignment faults, each of the six exception flags, each rounding mode, FZ, DN and AHP | `scripts/emu-difftest.py --fpu` |
| Replay (M3) | 10 polling programs recorded on QEMU replay with every peripheral load and the end state matching; QEMU's own trace of each equals the replay's instruction for instruction; byte flips and truncation rejected by the checksums, 42 re-signed corruptions caught by replay | `scripts/replay-test.py` |
| Replay with interrupts (M4) | 100 QEMU runs with nested interrupts from 3 sources at random rates replay exactly (every load, every interrupt at its recorded position, end registers and RAM); 10 crash recordings ended by a HardFault replay; 20 recordings whose interrupt keys are ambiguous replay by lookahead; QEMU's trace equals the replay's state for state, handlers included, also for an ambiguous one; IRQS byte flips rejected, 19 re-signed IRQS edits caught | `scripts/replay-irq-test.py` |
| Replay with interrupts on the Nucleo image | The NUCLEO-F411RE interrupt image (SysTick, TIM3, TIM4) records 20 runs and a crash run on QEMU netduinoplus2 and sends them over USART2; the bench script replays every one exactly | `scripts/qemu-nucleo-irq-smoke.py` |
| Capture stack bounds | The capture code (MPU-trap handler, interrupt stub, snapshot) uses no more stack than it zeroes, at -O0, -O2 and -Os; the stubs and replay agree with the macros | `scripts/check-stack-usage.py` |
| GDB sessions (M5) | 30 scripted gdb-multiarch sessions against `backstep gdb` on five QEMU recordings: registers equal the forward trace after steps, reverse steps and jumps; breakpoints, watchpoints (write, read, access), finish/reverse-finish, next/reverse-next, step/reverse-step; a reverse watchpoint stops on the store that corrupted memory (`demo_overrun.c`); GDB over TCP with `target extended-remote` (as IDEs connect); the protocol over TCP with no-ack mode and Ctrl-C | `scripts/gdb-test.py` |
| Queries (M6) | `backstep why` traces three example bugs recorded on QEMU to the source lines marked in them (an overrun in an interrupt handler, a task stack overflow, a lost update between main code and an interrupt); `backstep timeline` lists every recorded interrupt and a crash's fault, cause and the call before it; `--at`, raw addresses, lookup errors | `scripts/why-test.py` |
| Query logic | The lost-update rule (stale read, re-read, blind write, one context); `why` and the timeline on the checked-in fixture recording without debug information | `host/crates/backstep-query` |
| Timeline and protocol | Every route to a position (random jumps, steps back across interrupts and checkpoints) gives the forward replay's state; breakpoints and watchpoints in both directions; the GDB protocol packet by packet (from a checked-in recording, on every platform) | `host/crates/backstep-replay/tests/timeline.rs`, `host/crates/backstep-gdb/tests/protocol.rs` |
| Replay on the Nucleo image | The NUCLEO-F411RE recorder image, run on QEMU netduinoplus2, sends its recording over USART2; the bench script rebuilds and replays it; the replay equals QEMU's trace instruction for instruction | `scripts/qemu-nucleo-replay-smoke.py` |
| Recording format | Write/parse round trip; every single-bit flip and every truncation of a recording rejected | `host/crates/backstep-replay/tests` |
| Emulator coverage | Lines of the decoder, exception model and floating-point unit executed by the tests: at least 90% | `scripts/emu-coverage.sh` |
| Floating-point arithmetic | 300,000 random edge-biased operands per operation equal the host's IEEE results (round to nearest); flags, rounding modes, FZ, DN, fixed point and half precision on fixed cases | `host/crates/backstep-emu/src/fpu.rs` |
| Emulator unit tests | Stop conditions, UNPREDICTABLE encodings, alignment faults, branch offsets beyond the test image, privilege, stack-pointer banking, exclusive-monitor failures, rollback on faults, instruction-fetch bus errors, escalation, lockup, the replay interrupt API, unsupported cases, ELF loading, copy-on-write memory pages and watched accesses; for the FPU: NOCP before decoding, privileged-only access, UNDEFINED encodings, the floating-point context (FPCA, FPDSCR), alignment and a stopped VLDM | `host/crates/backstep-emu` |
| Hardware-in-the-loop | Real chip behaviour: clocks, timing, LED, replay vs real dumps | `scripts/hil/` |
| HIL script self-tests | The hardware scripts give correct verdicts (simulated board) | `scripts/hil/test_hil_scripts.py` |

## Running the tests

```bash
scripts/ci.sh            # everything
scripts/ci.sh rust       # host tools only
scripts/ci.sh recorder   # recorder unit tests
scripts/ci.sh firmware   # firmware builds, QEMU tests, HIL script self-test
```

## How QEMU tests report results

Test firmware prints `PASS <name>` / `FAIL <name>` lines on the emulated
UART, then `RESULT PASS` or `RESULT FAIL`, and exits QEMU through Arm
semihosting with a matching exit code. `scripts/qemu-run.sh` requires both
a zero exit code and the `RESULT PASS` line, and enforces a timeout.

## Hardware tests

Scripts in `scripts/hil/` drive a real board over its serial port and
print a report block to paste into the pull request or into
[HARDWARE_VERIFICATION.md](HARDWARE_VERIFICATION.md). Each script has a
self-test that replays good and bad serial output through a
pseudo-terminal, so its verdicts are themselves verified in CI.
