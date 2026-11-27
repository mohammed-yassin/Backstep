#!/usr/bin/env bash
# Run a test firmware ELF on QEMU mps2-an386 and propagate its result.
#
# The firmware prints to the emulated UART (our stdout) and exits through
# semihosting: exit status 0 = PASS, anything else = FAIL. A timeout guards
# against firmware that hangs.
#
# Usage: scripts/qemu-run.sh <firmware.elf> [timeout-seconds]
#
# SPDX-License-Identifier: MIT
set -euo pipefail

elf="${1:?usage: qemu-run.sh <firmware.elf> [timeout-seconds]}"
limit="${2:-20}"
qemu="${QEMU:-qemu-system-arm}"

if [[ ! -f "$elf" ]]; then
    echo "qemu-run: no such file: $elf" >&2
    exit 2
fi

set +e
output="$(timeout "$limit" "$qemu" \
    -machine mps2-an386 -cpu cortex-m4 \
    -nographic -monitor none -serial stdio \
    -semihosting-config enable=on,target=native \
    -kernel "$elf" 2>&1)"
status=$?
set -e

printf '%s\n' "$output"

if [[ $status -eq 124 ]]; then
    echo "qemu-run: TIMEOUT after ${limit}s: $elf" >&2
    exit 1
fi
if [[ $status -ne 0 ]]; then
    echo "qemu-run: FAIL (exit $status): $elf" >&2
    exit 1
fi
# Belt and braces: the firmware must also have printed its verdict.
if ! grep -q '^RESULT PASS' <<<"$output"; then
    echo "qemu-run: FAIL (no 'RESULT PASS' line): $elf" >&2
    exit 1
fi
echo "qemu-run: PASS: $elf"
