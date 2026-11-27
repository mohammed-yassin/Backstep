#!/usr/bin/env python3
"""M1 hardware-in-the-loop test: MPU peripheral-read trap on NUCLEO-F411RE.

Runs firmware/build/nucleo-f411re/mmio_trap_hw.bin, which checks every
trapped load/store form against the untrapped result on real STM32F411
registers, captures loads from interrupts at two priorities, prints while
all UART accesses are trapped, and measures the cost of a trapped load with
the DWT cycle counter.

Steps
  1. Build:  make -C firmware
  2. Flash:  copy firmware/build/nucleo-f411re/mmio_trap_hw.bin onto the
             NODE_F411RE USB drive (or use probe-rs download).
  3. Run:    python3 scripts/hil/m1_nucleo_f411re.py <serial-port>
  4. Press the black RESET button when asked; paste the report block.

SPDX-License-Identifier: MIT
"""

from __future__ import annotations

import argparse
import re
import sys
import time

BANNER = "BACKSTEP-M1 NUCLEO-F411RE MPU trap test"
MEASURE = re.compile(r"^MEASURE cycles_per_1000_loads trapped=(\d+) plain=(\d+) "
                     r"extra_cycles_per_trapped_load=(\d+)$")
# A trapped load costs two exception transitions plus decode and emulation.
# Anything outside this band means the measurement or the trap is broken.
EXTRA_CYCLES_MIN = 50
EXTRA_CYCLES_MAX = 2000


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("port", help="serial port of the Nucleo's ST-LINK VCP")
    parser.add_argument("--timeout", type=float, default=30.0,
                        help="seconds to wait for the test to finish")
    args = parser.parse_args()
    try:
        import serial  # type: ignore[import-not-found]
    except ImportError:
        print("pyserial is required: pip install pyserial", file=sys.stderr)
        return 2

    lines: list[str] = []
    started = False
    with serial.Serial(args.port, 115200, timeout=0.2) as port:
        port.reset_input_buffer()
        print(">>> Press the black RESET button on the Nucleo now.")
        deadline = time.monotonic() + args.timeout
        buffer = b""
        done = False
        while time.monotonic() < deadline and not done:
            buffer += port.read(256)
            while b"\n" in buffer:
                raw, buffer = buffer.split(b"\n", 1)
                line = raw.decode("ascii", errors="replace").strip()
                if not line:
                    continue
                print(f"    {line}")
                if line == BANNER:
                    started = True
                    lines.clear()
                    continue
                if started:
                    lines.append(line)
                    if line.startswith("RESULT "):
                        done = True

    checks = [(l[5:], True) for l in lines if l.startswith("PASS ")]
    checks += [(l[5:], False) for l in lines if l.startswith("FAIL ")]
    results: list[tuple[str, bool, str]] = [("test banner received", started, "")]
    results.append((f"{len(checks)} on-target checks, none failed",
                    bool(checks) and all(ok for _, ok in checks),
                    "; ".join(n for n, ok in checks if not ok)))
    measure = next((m for l in lines if (m := MEASURE.match(l))), None)
    if measure:
        extra = int(measure.group(3))
        results.append(("trap cost measured with DWT",
                        EXTRA_CYCLES_MIN <= extra <= EXTRA_CYCLES_MAX,
                        f"{extra} extra cycles per trapped load"))
    else:
        results.append(("trap cost measured with DWT", False, "no MEASURE line"))
    results.append(("firmware verdict RESULT PASS", "RESULT PASS" in lines, ""))

    ok = all(passed for _, passed, _ in results)
    print("\n----- copy from here -----")
    print("### M1 hardware test: MPU trap on NUCLEO-F411RE")
    for name, passed, detail in results:
        print(f"- [{'x' if passed else ' '}] {name}" + (f" ({detail})" if detail else ""))
    print(f"\n**Result: {'PASS' if ok else 'FAIL'}**")
    print("----- to here -----")
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
