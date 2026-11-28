#!/usr/bin/env python3
"""Check that the recorder's capture code fits in the dead stack it zeroes.

On the chip, the MPU-trap handler, bs_irq_record() and the snapshot's
save callback run below the program's stack; replay does not run them.
Each zeroes a fixed number of bytes below its stack top before returning
(ADR 0009), so replayed memory equals the chip's. That only holds if the
code never uses more stack than is zeroed. This script compiles the
recorder with GCC's call-graph and stack-usage output (-fcallgraph-info=su)
at -O0, -O2 and -Os and computes the deepest stack of each:

  MPU-trap handler   stub (40 bytes) + deepest path from the C handlers
                     <= BS_TRAP_STACK_BYTES
  interrupt stub     deepest path from bs_irq_record
                     <= BS_IRQ_STUB_SCRATCH_BYTES
  snapshot           8 bytes pushed + deepest path from record_save
                     <= BS_SNAPSHOT_SAVE_BYTES

Paths into stop_core() are left out: it ends the program (a genuine fault)
and never returns to it. Dynamic stack, recursion or a callee without
stack information fail the check.

Usage: check-stack-usage.py [--cc arm-none-eabi-gcc]

SPDX-License-Identifier: MIT
"""

from __future__ import annotations

import argparse
import re
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SOURCES = ["recorder/src/" + f for f in
           ("mpu_trap.c", "thumb_ldst.c", "irq_record.c", "record.c", "snapshot.c", "backstep.c")]
# memcpy/memset as the test firmware has them; a product's C library has
# its own, whose stack use adds to the snapshot's.
SOURCES.append("firmware/common/libc_min.c")
NO_RETURN = {"stop_core"}
TRAP_STUB_BYTES = 40  # push {r4-r11, lr} and the alignment word


def macro(header: str, name: str) -> int:
    text = (ROOT / "recorder" / "include" / header).read_text()
    return int(re.search(rf"#define {name} (\d+)u", text).group(1))


def call_graph(cc: str, opt: str, work: Path) -> tuple[dict[str, tuple[int, str]], dict[str, set[str]]]:
    sizes: dict[str, tuple[int, str]] = {}
    edges: dict[str, set[str]] = {}
    for src in SOURCES:
        obj = work / f"{opt}-{Path(src).name}.o"
        subprocess.run([cc, "-mcpu=cortex-m4", "-mthumb", "-mfloat-abi=soft", "-std=c99", f"-{opt}",
                        "-ffreestanding", "-fcallgraph-info=su",
                        "-I", str(ROOT / "recorder" / "include"), "-I", str(ROOT / "recorder" / "src"),
                        "-c", str(ROOT / src), "-o", str(obj)], check=True)
        text = obj.with_suffix(".ci").read_text()
        for title, label in re.findall(r'node: \{ title: "([^"]+)" label: "([^"]+)"', text):
            m = re.search(r"\\n(\d+) bytes \((\w+)\)", label)
            if m:
                sizes[title.split(":")[-1]] = (int(m.group(1)), m.group(2))
        for a, b in re.findall(r'edge: \{ sourcename: "([^"]+)" targetname: "([^"]+)"', text):
            edges.setdefault(a.split(":")[-1], set()).add(b.split(":")[-1])
    return sizes, edges


def deepest(fn: str, sizes, edges, stack: tuple[str, ...] = ()) -> tuple[int, list[str]]:
    if fn in stack:
        raise ValueError(f"recursion: {' -> '.join(stack + (fn,))}")
    if fn not in sizes:
        raise ValueError(f"no stack information for {fn} (called from {stack[-1] if stack else '-'})")
    own, kind = sizes[fn]
    if kind != "static":
        raise ValueError(f"{fn} uses {kind} stack")
    best, path = 0, []
    for callee in sorted(edges.get(fn, ())):
        if callee in NO_RETURN:
            continue
        depth, sub = deepest(callee, sizes, edges, stack + (fn,))
        if depth > best:
            best, path = depth, sub
    return own + best, [fn] + path


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--cc", default="arm-none-eabi-gcc")
    args = ap.parse_args()
    checks = [
        ("MPU-trap handler", ["bs_mmio_trap_memmanage", "bs_mmio_trap_hardfault"], TRAP_STUB_BYTES,
         macro("backstep_mmio.h", "BS_TRAP_STACK_BYTES"), "BS_TRAP_STACK_BYTES"),
        ("interrupt stub", ["bs_irq_record"], 0,
         macro("backstep_irq.h", "BS_IRQ_STUB_SCRATCH_BYTES"), "BS_IRQ_STUB_SCRATCH_BYTES"),
        ("snapshot save", ["record_save"], 8,
         macro("backstep_snapshot.h", "BS_SNAPSHOT_SAVE_BYTES"), "BS_SNAPSHOT_SAVE_BYTES"),
    ]
    failures = 0
    # The stubs' assembly must zero exactly what the macros (and replay) say.
    irq_stub = (ROOT / "recorder/include/backstep_irq.h").read_text().count('"stmdb r3!, {r0-r2, r12}')
    trap_stub = int(re.search(r'"sub r1, r0, #(\d+)', (ROOT / "recorder/src/mpu_trap.c").read_text()).group(1))
    replay = int(re.search(r"const TRAP_STACK_BYTES: u32 = (\d+);",
                           (ROOT / "host/crates/backstep-replay/src/replay.rs").read_text()).group(1))
    for name, zeroed, limit in (("interrupt stub zeroes", 16 * irq_stub, checks[1][3]),
                                ("MPU-trap stub zeroes", trap_stub + TRAP_STUB_BYTES, checks[0][3]),
                                ("replay's TRAP_STACK_BYTES", replay, checks[0][3])):
        ok = zeroed == limit
        failures += not ok
        print(f"{'PASS' if ok else 'FAIL'} {name} {zeroed} bytes, the macro says {limit}")
    with tempfile.TemporaryDirectory() as tmp:
        for opt in ("O0", "O2", "Os"):
            sizes, edges = call_graph(args.cc, opt, Path(tmp))
            for name, roots, extra, limit, macro_name in checks:
                try:
                    depth, path = max(deepest(r, sizes, edges) for r in roots)
                except ValueError as e:
                    print(f"FAIL {name} at -{opt}: {e}")
                    failures += 1
                    continue
                total = extra + depth
                ok = total <= limit
                failures += not ok
                print(f"{'PASS' if ok else 'FAIL'} {name} at -{opt}: {total} bytes "
                      f"<= {macro_name} ({limit}); deepest {' -> '.join(path)}")
    print(f"check-stack-usage: {'PASS' if failures == 0 else 'FAIL'}")
    return 0 if failures == 0 else 1


if __name__ == "__main__":
    sys.exit(main())
