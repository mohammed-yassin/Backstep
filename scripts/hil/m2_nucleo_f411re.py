#!/usr/bin/env python3
"""M2 hardware-in-the-loop measurement: the STM32F411's exclusive monitor.

Runs firmware/build/nucleo-f411re/exclusive_hw.bin, which checks the two
results the architecture requires (LDREX/STREX succeeds; CLREX in between
makes it fail) and measures four IMPLEMENTATION DEFINED ones: a STREX to
another address, of another size, after a plain store, and after an
exception. The replay emulator must decide every STREX as the chip does;
the report says which emulator setting matches this chip.

Steps
  1. Build:  make -C firmware
  2. Flash:  copy firmware/build/nucleo-f411re/exclusive_hw.bin onto the
             NODE_F411RE USB drive (or use probe-rs download).
  3. Run:    python3 scripts/hil/m2_nucleo_f411re.py <serial-port>
  4. Press the black RESET button when asked; paste the report block.

SPDX-License-Identifier: MIT
"""

from __future__ import annotations

import argparse
import re
import sys
import time

BANNER = "BACKSTEP-M2 NUCLEO-F411RE exclusive monitor test"
MEASURE = re.compile(r"^MEASURE excl (\w+) status=([01])$")
CASES = ["other_address", "other_size", "plain_store", "exception"]
# What the emulator does by default (and QEMU does): status per case.
EMULATOR_DEFAULT = {"other_address": 1, "other_size": 0, "plain_store": 0, "exception": 1}


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
    measured = {m.group(1): int(m.group(2)) for l in lines if (m := MEASURE.match(l))}
    results: list[tuple[str, bool, str]] = [("test banner received", started, "")]
    results.append((f"{len(checks)} required checks, none failed",
                    len(checks) >= 3 and all(ok for _, ok in checks),
                    "; ".join(n for n, ok in checks if not ok)))
    missing = [c for c in CASES if c not in measured]
    results.append(("all four cases measured", not missing,
                    f"missing: {', '.join(missing)}" if missing else ""))
    results.append(("firmware verdict RESULT PASS", "RESULT PASS" in lines, ""))

    ok = all(passed for _, passed, _ in results)
    print("\n----- copy from here -----")
    print("### M2 hardware measurement: exclusive monitor on NUCLEO-F411RE")
    for name, passed, detail in results:
        print(f"- [{'x' if passed else ' '}] {name}" + (f" ({detail})" if detail else ""))
    if not missing:
        print("\n| Case | STREX status on this chip | Emulator default |")
        print("|------|---------------------------|------------------|")
        for case in CASES:
            print(f"| {case} | {measured[case]} | {EMULATOR_DEFAULT[case]} |")
        diff = [c for c in CASES if measured[c] != EMULATOR_DEFAULT[c]]
        if not diff:
            print("\nThe emulator's default monitor matches this chip.")
        elif diff == ["other_address"]:
            print("\nThis chip's monitor does not compare addresses: replay of STM32F4 "
                  "recordings must set `Cpu::exclusive_tags_address = false`.")
        else:
            print(f"\nThe emulator differs from this chip in: {', '.join(diff)}.")
    print(f"\n**Result: {'PASS' if ok else 'FAIL'}**")
    print("----- to here -----")
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
