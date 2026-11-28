#!/usr/bin/env python3
"""M4 hardware-in-the-loop test: interrupts recorded on NUCLEO-F411RE,
replayed on the PC, 20 of 20 runs exact.

Runs firmware/build/nucleo-f411re/record_irq_hw.bin. The board records 20
runs of code interrupted at random rates by SysTick, TIM3 and TIM4 (three
priorities, nested), then a 21st run that ends in a crash (a HardFault),
and sends each recording over the ST-LINK virtual COM port as hex. This
script rebuilds each recording and runs `backstep replay` on it: every
interrupt must be delivered at its recorded position, every peripheral
load reproduced, and the end registers and RAM checksum must equal the
board's.

Steps
  1. Build:  make -C firmware ; (cd host && cargo build --release)
  2. Flash:  copy firmware/build/nucleo-f411re/record_irq_hw.bin onto the
             NODE_F411RE USB drive (or use probe-rs download).
  3. Run:    python3 scripts/hil/m4_nucleo_f411re.py <serial-port>
  4. Press the black RESET button when asked (the runs take about two
     minutes at 115200 baud); paste the report block.

`--from-file <capture>` reads a saved console capture instead of a port
(used by scripts/qemu-nucleo-irq-smoke.py with QEMU's netduinoplus2).
On the board every run must be interrupted by all three sources;
`--sources-overall` asks that only of all runs together, for QEMU, whose
STM32 timer model fires TIM3 and TIM4 mostly in the first run.

SPDX-License-Identifier: MIT
"""

from __future__ import annotations

import argparse
import importlib.util
import struct
import subprocess
import sys
from collections import Counter
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
BANNER = "BACKSTEP-M4 NUCLEO-F411RE interrupt record and replay test"
RUNS = 20
CRASH_RUN = RUNS + 1
SOURCES = {15: "SysTick", 16 + 29: "TIM3", 16 + 30: "TIM4"}

_spec = importlib.util.spec_from_file_location("m3_nucleo", HERE / "m3_nucleo_f411re.py")
m3 = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(m3)  # type: ignore[union-attr]


def runs(lines: list[str]) -> dict[int, list[str]]:
    """Console lines of each run, by run number."""
    out: dict[int, list[str]] = {}
    current = None
    for line in lines:
        if line.startswith("BSREC RUN "):
            try:
                current = int(line.split()[2])
            except (IndexError, ValueError):
                current = None
                continue
            out[current] = []
        elif current is not None:
            out[current].append(line)
            if line.startswith("BSREC END ") or line.startswith("BSREC NONE"):
                current = None
    return out


def interrupts(data: bytes) -> list[tuple[int, ...]]:
    """The IRQS events of a recording (ADR 0008): 20 words each."""
    pos = 16
    for _ in range(struct.unpack_from("<I", data, 12)[0]):
        kind, length = data[pos:pos + 4], struct.unpack_from("<I", data, pos + 4)[0]
        if kind == b"IRQS":
            count = struct.unpack_from("<I", data, pos + 8)[0]
            return [struct.unpack_from("<20I", data, pos + 20 + 80 * i) for i in range(count)]
        pos += 12 + length
    return []


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("port", nargs="?", help="serial port of the Nucleo's ST-LINK VCP")
    parser.add_argument("--from-file", help="read a saved console capture instead of a port")
    parser.add_argument("--backstep", default=str(ROOT / "host/target/release/backstep"))
    parser.add_argument("--elf", default=str(ROOT / "firmware/build/nucleo-f411re/record_irq_hw.elf"))
    parser.add_argument("--out-dir", default=".", help="where to save run-<n>.bsr")
    parser.add_argument("--timeout", type=float, default=600.0)
    parser.add_argument("--sources-overall", action="store_true",
                        help="require the three sources over all runs, not in each (QEMU)")
    args = parser.parse_args()
    if args.from_file:
        lines = [l.strip() for l in Path(args.from_file).read_text(errors="replace").splitlines()]
    elif args.port:
        m3.BANNER = BANNER
        lines = m3.read_port(args.port, args.timeout)
    else:
        parser.error("give a serial port or --from-file")

    results: list[tuple[str, bool, str]] = []
    results.append(("test banner received", BANNER in lines, ""))
    by_run = runs(lines)
    out_dir = Path(args.out_dir)
    out_dir.mkdir(parents=True, exist_ok=True)
    sources: Counter[int] = Counter()
    nested = total = 0
    exact: list[int] = []
    partial: list[str] = []  # runs not interrupted by every source
    failures: list[str] = []
    crash_detail, crash_ok = "not received", False
    for n in range(1, CRASH_RUN + 1):
        block = by_run.get(n)
        if block is None:
            failures.append(f"run {n}: not received")
            continue
        none = next((l for l in block if l.startswith("BSREC NONE")), None)
        if none is not None:
            failures.append(f"run {n}: the board could not record it ({none})")
            continue
        data, detail = m3.extract(block)
        if data is None:
            failures.append(f"run {n}: {detail}")
            continue
        rec = out_dir / f"run-{n}.bsr"
        rec.write_bytes(data)
        r = subprocess.run([args.backstep, "replay", args.elf, str(rec)], capture_output=True, text=True)
        summary = (r.stdout if r.returncode == 0 else r.stderr).strip().splitlines()
        first = summary[0] if summary else f"exit {r.returncode}"
        if n == CRASH_RUN:
            crash_ok, crash_detail = r.returncode == 0, first
            continue
        if r.returncode != 0:
            failures.append(f"run {n}: {first}")
            continue
        exact.append(n)
        evs = interrupts(data)
        missing = set(SOURCES) - {e[0] for e in evs}
        if missing:
            partial.append(f"run {n}: no " + ", ".join(SOURCES[k] for k in sorted(missing)))
        sources.update(e[0] for e in evs)
        nested += sum(e[17] & 0x1FF != 0 for e in evs)
        total += len(evs)

    results.append((f"{len(exact)} of {RUNS} runs replay exactly (every interrupt at its position, every "
                    "load, end registers and RAM CRC)", len(exact) == RUNS,
                    f"{total} interrupts, {nested} nested"))
    seen = ", ".join(f"{SOURCES.get(k, k)} {v}" for k, v in sorted(sources.items()))
    if args.sources_overall:
        results.append(("all three sources interrupted (over all runs), and nested",
                        set(sources) == set(SOURCES) and nested > 0, seen))
    else:
        results.append(("every run interrupted by all three sources, and nested",
                        len(exact) == RUNS and not partial and nested > 0,
                        "; ".join(partial) if partial else seen))
    crashed = any(l.startswith("crash at ") for l in lines)
    results.append(("run 21 crashed (HardFault) and its recording replays exactly", crashed and crash_ok,
                    crash_detail if crashed else "no crash reported"))
    results.append(("firmware verdict RESULT PASS", "RESULT PASS" in lines, ""))

    ok = all(passed for _, passed, _ in results)
    print("\n----- copy from here -----")
    print("### M4 hardware test: interrupts recorded on NUCLEO-F411RE, replayed on the PC")
    for name, passed, d in results:
        print(f"- [{'x' if passed else ' '}] {name}" + (f" ({d})" if d else ""))
    if failures:
        print("\n```\n" + "\n".join(failures) + "\n```")
    print(f"\n**Result: {'PASS' if ok else 'FAIL'}**")
    print("----- to here -----")
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
