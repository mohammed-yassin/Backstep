#!/usr/bin/env bash
# Line coverage of the emulator's decoder, exception model and floating-
# point unit, from the unit tests plus differential runs against QEMU
# (integer and floating point), with an instrumented build
# (-C instrument-coverage). M2 requires at least 90%.
#
# Usage: scripts/emu-coverage.sh [--programs N] [--min PERCENT]
# Needs llvm-profdata and llvm-cov that read rustc's profile format: the
# rustup component llvm-tools(-preview), or LLVM_TOOLS=<dir> (LLVM >= 20).
#
# SPDX-License-Identifier: MIT
set -euo pipefail
cd "$(dirname "$0")/.."
programs=40
min=90
while [[ $# -gt 0 ]]; do
    case "$1" in
        --programs) programs="$2"; shift 2 ;;
        --min) min="$2"; shift 2 ;;
        *) echo "unknown argument $1" >&2; exit 2 ;;
    esac
done

tools="${LLVM_TOOLS:-}"
if [[ -z "$tools" ]]; then
    for dir in "$(rustc --print sysroot)/lib/rustlib/$(rustc -vV | sed -n 's/^host: //p')/bin" \
               /usr/lib/llvm-22/bin /usr/lib/llvm-21/bin /usr/lib/llvm-20/bin; do
        if [[ -x "$dir/llvm-profdata" && -x "$dir/llvm-cov" ]]; then tools="$dir"; break; fi
    done
fi
if [[ -z "$tools" ]]; then
    echo "emu-coverage: no llvm-profdata/llvm-cov (rustup component add llvm-tools-preview)" >&2
    exit 2
fi

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
export RUSTFLAGS="-C instrument-coverage"
export CARGO_TARGET_DIR="$PWD/host/target/coverage"
export LLVM_PROFILE_FILE="$tmp/profile-%p-%m.profraw"

(cd host && cargo build --locked --release -q -p backstep-emu)
(cd host && cargo test --locked --release -q -p backstep-emu >/dev/null)
tests="$(cd host && cargo test --locked --release -q -p backstep-emu --no-run --message-format=json \
    | python3 -c 'import json,sys
for l in sys.stdin:
    m = json.loads(l)
    if m.get("reason") == "compiler-artifact" and m.get("executable") and m["profile"]["test"]:
        print(m["executable"])')"
python3 scripts/emu-difftest.py --emu "$CARGO_TARGET_DIR/release/backstep-emu-trace" \
    --programs "$programs" --seed 3 --jobs "$(nproc)" | tail -1
python3 scripts/emu-difftest.py --emu "$CARGO_TARGET_DIR/release/backstep-emu-trace" --fpu \
    --programs "$programs" --seed 3 --jobs "$(nproc)" | tail -1

"$tools/llvm-profdata" merge -sparse -o "$tmp/merged.profdata" "$tmp"/profile-*.profraw
objects=("$CARGO_TARGET_DIR/release/backstep-emu-trace")
for t in $tests; do objects+=("-object=$t"); done
files=(crates/backstep-emu/src/cpu.rs crates/backstep-emu/src/cpu/thumb32.rs
       crates/backstep-emu/src/cpu/exceptions.rs crates/backstep-emu/src/cpu/vfp.rs
       crates/backstep-emu/src/fpu.rs)
report="$(cd host && "$tools/llvm-cov" report -instr-profile="$tmp/merged.profdata" \
    "${objects[@]}" "${files[@]}")"
echo "$report"
# Columns: Filename Regions Missed Cover Functions Missed Executed Lines Missed Cover ...
total="$(awk '$1 == "TOTAL" {print $10}' <<<"$report" | tr -d '%')"
if awk -v t="$total" -v m="$min" 'BEGIN {exit !(t >= m)}'; then
    echo "emu-coverage: PASS (decoder, exception model and FPU: ${total}% of lines, minimum ${min}%)"
else
    echo "emu-coverage: FAIL (decoder, exception model and FPU: ${total}% of lines, minimum ${min}%)"
    exit 1
fi
