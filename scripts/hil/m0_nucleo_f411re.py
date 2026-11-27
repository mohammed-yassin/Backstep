#!/usr/bin/env python3
"""M0 hardware-in-the-loop test for NUCLEO-F411RE.

Checks on real hardware what emulation cannot:
  * the firmware boots from flash on the real chip,
  * USART2 works at 115200 baud through the ST-LINK virtual COM port,
  * the 16 MHz HSI clock and 1 ms SysTick give 1.000 s heartbeats,
  * the green LED (LD2) blinks (confirmed by you, by eye).

Steps
  1. Build:   make -C firmware
  2. Flash:   copy firmware/build/nucleo-f411re/blinky.bin onto the
              NODE_F411RE USB drive that appears when the Nucleo is plugged
              in (or: probe-rs download --chip STM32F411RETx
              firmware/build/nucleo-f411re/blinky.elf).
  3. Run:     python3 scripts/hil/m0_nucleo_f411re.py <serial-port>
              e.g. /dev/ttyACM0 (Linux), /dev/cu.usbmodem* (macOS), COM5
              (Windows). Needs pyserial: pip install pyserial
  4. Press the black RESET button on the Nucleo when asked.
  5. Paste the final report block into the pull request or issue.

SPDX-License-Identifier: MIT
"""

from __future__ import annotations

import argparse
import re
import sys
import time

BANNER = re.compile(
    r"^BACKSTEP-M0 NUCLEO-F411RE blinky recorder=(\d+\.\d+\.\d+) crc=([0-9A-F]{8})$"
)
BEAT = re.compile(r"^BACKSTEP-M0 alive (\d+)$")
HEARTBEATS_REQUIRED = 5
# HSI is factory-trimmed to +/-1 % at 25 C; allow 2 % plus host-side jitter.
PERIOD_MIN_S = 0.95
PERIOD_MAX_S = 1.05


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("port", help="serial port of the Nucleo's ST-LINK VCP")
    parser.add_argument("--timeout", type=float, default=20.0,
                        help="seconds to wait for banner and heartbeats")
    args = parser.parse_args()

    try:
        import serial  # type: ignore[import-not-found]
    except ImportError:
        print("pyserial is required: pip install pyserial", file=sys.stderr)
        return 2

    results: list[tuple[str, bool, str]] = []

    with serial.Serial(args.port, 115200, timeout=0.2) as port:
        port.reset_input_buffer()
        print(">>> Press the black RESET button on the Nucleo now.")
        deadline = time.monotonic() + args.timeout
        banner = None
        beats: list[tuple[int, float]] = []
        buffer = b""
        while time.monotonic() < deadline and len(beats) < HEARTBEATS_REQUIRED:
            buffer += port.read(256)
            while b"\n" in buffer:
                raw, buffer = buffer.split(b"\n", 1)
                line = raw.decode("ascii", errors="replace").strip()
                if not line:
                    continue
                print(f"    {line}")
                if m := BANNER.match(line):
                    banner = m
                    beats.clear()
                elif (m := BEAT.match(line)) and banner is not None:
                    beats.append((int(m.group(1)), time.monotonic()))

    results.append(("banner received", banner is not None, ""))
    if banner is not None:
        results.append(("CRC-32 check value on target", banner.group(2) == "CBF43926",
                        banner.group(2)))
        results.append(("recorder version reported", True, banner.group(1)))

    numbers = [n for n, _ in beats]
    results.append((f"{HEARTBEATS_REQUIRED} heartbeats received",
                    len(beats) >= HEARTBEATS_REQUIRED, str(numbers)))
    results.append(("heartbeats numbered 1..N without gaps",
                    numbers == list(range(1, len(numbers) + 1)) and len(numbers) > 0, ""))

    if len(beats) >= 3:
        periods = [b[1] - a[1] for a, b in zip(beats, beats[1:])]
        mean = sum(periods) / len(periods)
        results.append(("heartbeat period ~1.000 s (clock + SysTick correct)",
                        PERIOD_MIN_S <= mean <= PERIOD_MAX_S, f"mean {mean:.3f} s"))

    answer = input(">>> Is the green LED LD2 blinking on/off every 0.5 s? [y/n] ")
    results.append(("LED LD2 blinks at 1 Hz (visual check)",
                    answer.strip().lower().startswith("y"), ""))

    ok = all(passed for _, passed, _ in results)
    print("\n----- copy from here -----")
    print("### M0 hardware test: NUCLEO-F411RE")
    for name, passed, detail in results:
        print(f"- [{'x' if passed else ' '}] {name}" + (f" ({detail})" if detail else ""))
    print(f"\n**Result: {'PASS' if ok else 'FAIL'}**")
    print("----- to here -----")
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
