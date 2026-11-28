#!/usr/bin/env python3
"""Check that the recorder's code in a hard-float firmware image never
touches the floating-point registers.

The recorder runs inside the firmware's MPU-trap handler and interrupt
stubs, which replay reproduces without running them (docs/adr/0012). If
that code executed a floating-point instruction, the core would preserve
the interrupted code's FP state lazily, writing memory replay cannot
predict. recorder/src/bs_no_fpu.h makes GCC use core registers only; this
check proves it on the built image: every function defined in
recorder/src (found with nm -l from the debug information) is
disassembled, and any floating-point instruction fails the check, except
in bs_snapshot_take, which reads and reloads the FP registers on purpose
(outside any exception).

Usage: check-recorder-no-fpu.py <firmware.elf>...

SPDX-License-Identifier: MIT
"""

from __future__ import annotations

import re
import subprocess
import sys

ALLOWED = {"bs_snapshot_take"}
FP_INSN = re.compile(r"\tv[a-z]+")


def recorder_functions(elf: str) -> set[str]:
    out = subprocess.run(["arm-none-eabi-nm", "-l", "--defined-only", elf], capture_output=True,
                         text=True, check=True).stdout
    names = set()
    for line in out.splitlines():
        parts = line.split()
        if len(parts) >= 4 and parts[1] in "tT" and "recorder/src/" in parts[3]:
            names.add(parts[2])
    return names


def fp_functions(elf: str) -> dict[str, int]:
    dis = subprocess.run(["arm-none-eabi-objdump", "-d", elf], capture_output=True, text=True,
                         check=True).stdout
    counts: dict[str, int] = {}
    current = ""
    for line in dis.splitlines():
        m = re.match(r"^[0-9a-f]+ <(.+)>:$", line)
        if m:
            current = m.group(1)
        elif FP_INSN.search(line):
            counts[current] = counts.get(current, 0) + 1
    return counts


def main() -> int:
    if len(sys.argv) < 2:
        print(__doc__)
        return 2
    failures = 0
    for elf in sys.argv[1:]:
        ours = recorder_functions(elf)
        if not ours:
            print(f"FAIL {elf}: no recorder functions found (built without -g?)")
            failures += 1
            continue
        bad = {f: n for f, n in fp_functions(elf).items() if f in ours and f not in ALLOWED}
        if bad:
            print(f"FAIL {elf}: floating-point instructions in recorder code: "
                  + ", ".join(f"{f} ({n})" for f, n in sorted(bad.items())))
            failures += 1
        else:
            print(f"PASS {elf}: {len(ours)} recorder functions, no floating-point instructions "
                  "outside bs_snapshot_take")
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
