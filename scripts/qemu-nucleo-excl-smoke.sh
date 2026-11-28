#!/usr/bin/env bash
# Smoke-test the NUCLEO-F411RE exclusive-monitor image on QEMU netduinoplus2
# (STM32F405) before it reaches a board: it must boot, pass the two
# architecturally required cases, print all four measurements and reach its
# verdict. QEMU's own measurements are printed for reference; the board's
# are collected by scripts/hil/m2_nucleo_f411re.py.
#
# SPDX-License-Identifier: MIT
set -euo pipefail
elf="${1:?usage: qemu-nucleo-excl-smoke.sh <exclusive_hw.elf>}"
qemu="${QEMU:-qemu-system-arm}"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

timeout 10 "$qemu" -machine netduinoplus2 -nographic -monitor none \
    -serial null -serial "file:$tmp/usart2.txt" -kernel "$elf" >/dev/null 2>&1 || true
out="$(tr -d '\r' <"$tmp/usart2.txt")"
failures=0
need() {
    if grep -qxE -- "$1" <<<"$out"; then
        echo "PASS $2"
    else
        echo "FAIL $2 (no line matching '$1')"; failures=$((failures + 1))
    fi
}
need "BACKSTEP-M2 NUCLEO-F411RE exclusive monitor test" "banner"
need "PASS ldrex A; strex A succeeds" "LDREX/STREX pair succeeds"
need "PASS ldrex A; clrex; strex A fails" "CLREX makes STREX fail"
need "PASS PendSV taken between ldrex and strex" "exception case ran"
for case in other_address other_size plain_store exception; do
    need "MEASURE excl $case status=[01]" "measured $case"
done
need "RESULT PASS" "verdict"
echo "QEMU's monitor: $(grep '^MEASURE' <<<"$out" | sed 's/MEASURE excl //' | tr '\n' ' ')"
if [[ $failures -ne 0 ]]; then
    printf -- '--- USART2 ---\n%s\n' "$out"; echo "qemu-nucleo-excl-smoke: FAIL"; exit 1
fi
echo "qemu-nucleo-excl-smoke: PASS"
