#!/usr/bin/env bash
# Run every automated check, exactly as CI does. A pull request is only
# merged when this script passes locally *and* in GitHub Actions.
#
# Usage: scripts/ci.sh            (all checks)
#        scripts/ci.sh rust|recorder|firmware
#
# SPDX-License-Identifier: MIT
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"
what="${1:-all}"

step() { printf '\n==> %s\n' "$*"; }

run_rust() {
    step "Rust: formatting"
    (cd host && cargo fmt --all -- --check)
    step "Rust: clippy (warnings are errors)"
    (cd host && cargo clippy --locked --all-targets -- -D warnings)
    step "Rust: tests"
    (cd host && cargo test --locked)
    step "Rust: docs build without warnings"
    (cd host && RUSTDOCFLAGS="-D warnings" cargo doc --locked --no-deps)
}

run_recorder() {
    step "Recorder: host unit tests (ASan + UBSan)"
    make -C recorder clean test
}

run_firmware() {
    step "Firmware: build all targets (warnings are errors)"
    make -C firmware clean all
    step "Recorder: capture code fits in the dead stack it zeroes (-O0, -O2, -Os)"
    python3 scripts/check-stack-usage.py
    step "Firmware: QEMU mps2-an386 tests (self-test, MPU trap, genuine-fault mode)"
    make -C firmware qemu-test
    step "Firmware: MPU trap vs Clang pass capture comparison"
    make -C firmware cc-test
    step "Firmware: 14,000 recorded interrupt positions vs QEMU instruction trace"
    make -C firmware irq-test
    step "Firmware: snapshot restored into a fresh machine continues identically"
    make -C firmware snapshot-test
    step "Firmware: NUCLEO-F411RE smoke test on QEMU netduinoplus2"
    scripts/qemu-nucleo-smoke.sh firmware/build/nucleo-f411re/blinky.elf
    step "Firmware: NUCLEO-F411RE MPU trap test runs end to end on QEMU netduinoplus2"
    scripts/qemu-nucleo-trap-smoke.sh firmware/build/nucleo-f411re/mmio_trap_hw.elf
    step "Firmware: NUCLEO-F411RE exclusive-monitor test runs end to end on QEMU netduinoplus2"
    scripts/qemu-nucleo-excl-smoke.sh firmware/build/nucleo-f411re/exclusive_hw.elf
    step "Firmware: host CLI agrees with recorder CRC on built image"
    local bin=firmware/build/nucleo-f411re/blinky.bin
    local ours theirs
    ours="$(cd host && cargo run --quiet --locked -p backstep-cli -- crc32 "../$bin" | cut -d' ' -f1)"
    theirs="$(python3 -c 'import sys,zlib; print("%08X" % zlib.crc32(open(sys.argv[1],"rb").read()))' "$bin")"
    if [[ "$ours" != "$theirs" ]]; then
        echo "CRC mismatch: backstep=$ours zlib=$theirs" >&2
        exit 1
    fi
    echo "PASS backstep crc32 == zlib crc32 ($ours)"
    step "Emulator: 1,000,000 instructions of random programs (Thumb, Thumb-2, exceptions) identical to QEMU, registers and memory"
    (cd host && cargo build --locked --release -p backstep-emu)
    python3 scripts/emu-difftest.py --emu host/target/release/backstep-emu-trace \
        --programs 460 --length 1500 --seed "${EMU_DIFFTEST_SEED:-1}" --jobs "$(nproc)" \
        --min-instructions 1000000
    step "Emulator: decoder and exception model line coverage >= 90%"
    scripts/emu-coverage.sh --min 90
    step "Replay: 10 recorded polling programs replayed instruction for instruction vs QEMU; corrupted recordings detected"
    (cd host && cargo build --locked --release -p backstep-cli)
    make -C firmware replay-test
    step "Replay: nested interrupts from 3 sources at random rates, 100 of 100 QEMU runs replay exactly; crashes; traces vs QEMU; corrupted IRQS detected"
    make -C firmware replay-irq-test
    step "GDB: 30 scripted sessions against backstep gdb (forwards and backwards, breakpoints, watchpoints, the demo bug)"
    make -C firmware gdb-test
    step "Replay: NUCLEO-F411RE image records itself on QEMU netduinoplus2, sends the recording over USART2, replays identically"
    python3 scripts/qemu-nucleo-replay-smoke.py --backstep host/target/release/backstep \
        --elf firmware/build/nucleo-f411re/record_hw.elf
    step "Replay: NUCLEO-F411RE interrupt image records 20 runs and a crash on QEMU netduinoplus2, all replay exactly"
    python3 scripts/qemu-nucleo-irq-smoke.py --backstep host/target/release/backstep \
        --elf firmware/build/nucleo-f411re/record_irq_hw.elf
    step "Hardware test scripts: self-test against a simulated board"
    python3 scripts/hil/test_hil_scripts.py
}

case "$what" in
    all) run_rust; run_recorder; run_firmware ;;
    rust) run_rust ;;
    recorder) run_recorder ;;
    firmware) run_firmware ;;
    *) echo "unknown check group: $what" >&2; exit 2 ;;
esac

step "All checks passed ($what)"
