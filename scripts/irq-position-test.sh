#!/usr/bin/env bash
# M1: check recorded interrupt positions against QEMU's instruction trace.
#
# Runs the interrupt-position firmware with several seeds, at -O2 and -O0,
# each time with QEMU logging every instruction's register state, and
# checks every recorded interrupt with irq-position-check.py.
# Total: 7 runs x 2000 = 14,000 interrupts.
#
# Usage: irq-position-test.sh <irq_position_test.elf> <irq_position_test_O0.elf>
#
# SPDX-License-Identifier: MIT
set -euo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
elf_o2="$(realpath "$1")"; elf_o0="$(realpath "$2")"
qemu="${QEMU:-qemu-system-arm}"
per_run=2000
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

totals=""
run() {
    local elf="$1" seed="$2" name="$3"
    (cd "$work" && rm -f irq_events.bin qemu.log &&
        timeout 300 "$qemu" -machine mps2-an386 -cpu cortex-m4 -nographic \
            -monitor none -serial stdio \
            -semihosting-config "enable=on,target=native,arg=$seed $per_run" \
            -icount shift=0 -accel tcg,one-insn-per-tb=on \
            -d cpu,int,nochain -D qemu.log -kernel "$elf" >run.txt 2>&1) || {
        cat "$work/run.txt"; echo "irq-position-test: QEMU run failed ($name seed $seed)"; exit 1; }
    grep -q '^RESULT PASS' "$work/run.txt" || {
        cat "$work/run.txt"; echo "irq-position-test: firmware failed ($name seed $seed)"; exit 1; }
    echo "--- $name, seed $seed"
    python3 "$here/irq-position-check.py" "$elf" "$work/qemu.log" "$work/irq_events.bin" \
        --expect-events "$per_run" | tee "$work/check.txt"
    totals+="$(grep '^STATS' "$work/check.txt")"$'\n'
    if [[ "$seed" == 1 ]]; then
        # Prove the checker detects a wrong record in this very trace.
        if python3 "$here/irq-position-check.py" "$elf" "$work/qemu.log" \
                "$work/irq_events.bin" --corrupt 1000 >/dev/null; then
            echo "irq-position-test: FAIL - a corrupted event was not detected"; exit 1
        fi
        echo "PASS corrupted event detected"
    fi
}

for seed in 1 2 3 4 5; do run "$elf_o2" "$seed" "-O2"; done
for seed in 6 7; do run "$elf_o0" "$seed" "-O0"; done

python3 - "$totals" <<'PY'
import sys
total = {}
for line in sys.argv[1].strip().splitlines():
    for kv in line.split()[1:]:
        k, v = kv.split("=")
        total[k] = total.get(k, 0) + int(v)
print("TOTAL " + " ".join(f"{k}={v}" for k, v in total.items()))
PY
echo "irq-position-test: PASS"
