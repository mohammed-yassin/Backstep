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
    step "Firmware: QEMU mps2-an386 self-test"
    scripts/qemu-run.sh firmware/build/qemu-mps2-an386/hello.elf
    step "Firmware: NUCLEO-F411RE smoke test on QEMU netduinoplus2"
    scripts/qemu-nucleo-smoke.sh firmware/build/nucleo-f411re/blinky.elf
    step "Firmware: host CLI agrees with recorder CRC on built image"
    local bin=firmware/build/nucleo-f411re/blinky.bin
    local ours theirs
    ours="$(cd host && cargo run --quiet --locked -- crc32 "../$bin" | cut -d' ' -f1)"
    theirs="$(python3 -c 'import sys,zlib; print("%08X" % zlib.crc32(open(sys.argv[1],"rb").read()))' "$bin")"
    if [[ "$ours" != "$theirs" ]]; then
        echo "CRC mismatch: backstep=$ours zlib=$theirs" >&2
        exit 1
    fi
    echo "PASS backstep crc32 == zlib crc32 ($ours)"
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
