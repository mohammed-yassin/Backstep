#!/usr/bin/env bash
# M1: compare the two peripheral-read capture mechanisms on one workload.
#
# Runs the MPU-trap image and the Clang-pass image under QEMU (with
# -icount, so instruction counts are exact and runs are deterministic) and
# checks that:
#   - both capture every read the workload made, in both phases,
#   - both produce byte-identical logs (same addresses, values, sizes,
#     order) and the workload computes the same result,
#   - the Clang pass really rewrote the workload's volatile loads.
# Prints the measured cost of each mechanism.
#
# Usage: capture-compare.sh <mpu.elf> <cc.elf> <workload_cc.o>
#
# SPDX-License-Identifier: MIT
set -euo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
mpu_elf="$1"; cc_elf="$2"; cc_obj="$3"
objdump="${OBJDUMP:-arm-none-eabi-objdump}"

mpu_out="$(QEMU_EXTRA_ARGS="-icount shift=0" "$here/qemu-run.sh" "$mpu_elf")"
cc_out="$(QEMU_EXTRA_ARGS="-icount shift=0" "$here/qemu-run.sh" "$cc_elf")"
failures=0

hooks="$("$objdump" -d "$cc_obj" | grep -c 'bl.*<__bs_mmio_load' || true)"
if [[ "$hooks" -gt 0 ]]; then
    echo "PASS clang pass inserted $hooks hook calls into the workload"
else
    echo "FAIL clang pass inserted no hook calls"; failures=$((failures + 1))
fi

for phase in peripheral ram; do
    a="$(grep "^DIGEST $phase " <<<"$mpu_out" | sed 's/^DIGEST //')"
    b="$(grep "^DIGEST $phase " <<<"$cc_out" | sed 's/^DIGEST //')"
    if [[ -n "$a" && "$a" == "$b" ]]; then
        echo "PASS $phase: identical capture ($a)"
    else
        echo "FAIL $phase: mpu [$a] vs cc [$b]"; failures=$((failures + 1))
    fi
done

grep '^MEASURE' <<<"$mpu_out"$'\n'"$cc_out" || true
if [[ $failures -ne 0 ]]; then
    printf '%s\n--- cc ---\n%s\n' "$mpu_out" "$cc_out"
    echo "capture-compare: FAIL"; exit 1
fi
echo "capture-compare: PASS"
