#!/usr/bin/env bash
# Smoke-test the NUCLEO-F411RE firmware in emulation before it ever reaches
# a real board.
#
# QEMU has no STM32F411 model, but its "netduinoplus2" board (STM32F405)
# shares the F411's memory map for flash, SRAM, RCC, GPIOA and USART2. QEMU
# leaves RCC and GPIO unimplemented and logs every access to them, so this
# script checks two things:
#   1. the firmware boots and prints its banner and heartbeats on USART2;
#   2. it programs the LED pin, USART2 pin, and peripheral clocks with the
#      exact register values the F411 needs.
# Timing and the physical LED are covered by the hardware test in
# scripts/hil/m0_nucleo_f411re.py.
#
# Usage: scripts/qemu-nucleo-smoke.sh <blinky.elf>
#
# SPDX-License-Identifier: MIT
set -euo pipefail

elf="${1:?usage: qemu-nucleo-smoke.sh <blinky.elf>}"
qemu="${QEMU:-qemu-system-arm}"
workdir="$(mktemp -d)"
trap 'rm -rf "$workdir"' EXIT

# USART1 -> discarded, USART2 -> file (the Nucleo's ST-LINK console).
timeout 5 "$qemu" -machine netduinoplus2 -nographic -monitor none \
    -serial null -serial "file:$workdir/usart2.txt" \
    -d guest_errors,unimp -D "$workdir/qemu.log" \
    -kernel "$elf" >/dev/null 2>&1 || true

console="$(tr -d '\r' <"$workdir/usart2.txt")"
log="$(cat "$workdir/qemu.log")"
failures=0

expect_console() {
    if grep -qE "$1" <<<"$console"; then
        echo "PASS console: $2"
    else
        echo "FAIL console: $2 (pattern: $1)"
        failures=$((failures + 1))
    fi
}

expect_write() {
    if grep -qF "$1" <<<"$log"; then
        echo "PASS register: $2"
    else
        echo "FAIL register: $2 (expected log entry: $1)"
        failures=$((failures + 1))
    fi
}

expect_console '^BACKSTEP-M0 NUCLEO-F411RE blinky recorder=[0-9]+\.[0-9]+\.[0-9]+ crc=CBF43926$' \
    "banner with recorder version and CRC check value"
expect_console '^BACKSTEP-M0 alive 1$' "first heartbeat"
expect_console '^BACKSTEP-M0 alive 2$' "second heartbeat"

expect_write "RCC: unimplemented device write (size 4, offset 0x030, value 0x00000001)" \
    "GPIOA clock enabled (RCC_AHB1ENR.GPIOAEN)"
expect_write "RCC: unimplemented device write (size 4, offset 0x040, value 0x00020000)" \
    "USART2 clock enabled (RCC_APB1ENR.USART2EN)"
expect_write "GPIOA: unimplemented device write (size 4, offset 0x000, value 0x00000420)" \
    "PA5 output + PA2 alternate function (GPIOA_MODER)"
expect_write "GPIOA: unimplemented device write (size 4, offset 0x020, value 0x00000700)" \
    "PA2 = AF7 USART2_TX (GPIOA_AFRL)"
expect_write "GPIOA: unimplemented device write (size 4, offset 0x014, value 0x00000020)" \
    "LED pin PA5 toggled (GPIOA_ODR)"

if [[ $failures -ne 0 ]]; then
    echo "--- USART2 output ---"
    printf '%s\n' "$console"
    echo "qemu-nucleo-smoke: FAIL ($failures checks)"
    exit 1
fi
echo "qemu-nucleo-smoke: PASS"
