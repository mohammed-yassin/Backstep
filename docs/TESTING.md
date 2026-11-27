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
| QEMU board smoke tests | Nucleo firmware programs the right registers, and the Nucleo trap test runs end to end, before reaching hardware | `scripts/qemu-nucleo-smoke.sh`, `scripts/qemu-nucleo-trap-smoke.sh` |
| Decoder vectors | The load/store decoder agrees with the GNU assembler on 1006 encodings; 2.6 M random encodings decode consistently | `recorder/tests/test_thumb_ldst.c`, `gen_ldst_vectors.py` |
| MPU trap tests (QEMU) | Every load/store form, IT blocks, interrupts at both priorities, genuine-fault detection | `firmware/targets/qemu-mps2-an386/mmio_trap_test.c` |
| Capture comparison | MPU trap and Clang pass capture byte-identical logs of the same workload | `scripts/capture-compare.sh` |
| Interrupt positions | 14,000 recorded interrupt contexts match QEMU's per-instruction trace; the key is unique or provably safe | `scripts/irq-position-test.sh`, `scripts/irq-position-check.py` |
| Snapshot restore | A snapshot restored into a fresh QEMU continues identically; leaving out any part changes the result | `scripts/snapshot-restore-test.sh`, `scripts/snapshot_tool.py` |
| Interrupt recorder logic | Context fields, alignment padding, stack hash, preemption before the stub's first instruction | `recorder/tests/test_irq_record.c` |
| Emulator differential test | Random Thumb programs: every register and xPSR identical to QEMU before every instruction; every instruction class executed | `scripts/emu-difftest.py` |
| Emulator unit tests | Stop conditions, UNPREDICTABLE encodings, rollback on faults, ELF loading | `host/crates/backstep-emu` |
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
