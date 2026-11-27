#!/usr/bin/env bash
# Smoke-test the NUCLEO-F411RE MPU trap test image on QEMU netduinoplus2
# (STM32F405) before it reaches a board.
#
# QEMU does not model GPIOB or the DWT cycle counter (reads return 0), so
# the value checks are expected to fail there. What this proves is that the
# image boots, the trap handles every access (including from interrupts at
# both priorities and the trapped console) without crashing or hanging, and
# the firmware reaches its verdict. Values are checked on real hardware by
# scripts/hil/m1_nucleo_f411re.py.
#
# SPDX-License-Identifier: MIT
set -euo pipefail
elf="${1:?usage: qemu-nucleo-trap-smoke.sh <mmio_trap_hw.elf>}"
qemu="${QEMU:-qemu-system-arm}"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

timeout 10 "$qemu" -machine netduinoplus2 -nographic -monitor none \
    -serial null -serial "file:$tmp/usart2.txt" -kernel "$elf" >/dev/null 2>&1 || true
out="$(tr -d '\r' <"$tmp/usart2.txt")"
failures=0
need() {
    local count
    count="$(grep -cxF -- "$1" <<<"$out" || true)"
    if [[ "$count" -ge "$2" ]]; then
        echo "PASS $3"
    else
        echo "FAIL $3 (expected '$1' x$2, saw x$count)"; failures=$((failures + 1))
    fi
}
need "BACKSTEP-M1 NUCLEO-F411RE MPU trap test" 1 "banner"
need "PASS MPU present, trap enabled" 1 "MPU detected on the STM32F4 model"
need "PASS   ...logged once with address and size" 10 "all 10 load forms trapped and logged"
need "PASS   ...every load captured" 2 "interrupt captures at both priorities"
need "PASS console works while trapped (SR polls captured)" 1 "trapped console"
if grep -qE '^MEASURE cycles_per_1000_loads ' <<<"$out" && grep -qE '^RESULT (PASS|FAIL)$' <<<"$out"; then
    echo "PASS reached measurement and verdict"
else
    echo "FAIL did not reach the end of the test"; failures=$((failures + 1))
fi
if [[ $failures -ne 0 ]]; then
    printf -- '--- USART2 ---\n%s\n' "$out"; echo "qemu-nucleo-trap-smoke: FAIL"; exit 1
fi
echo "qemu-nucleo-trap-smoke: PASS"
