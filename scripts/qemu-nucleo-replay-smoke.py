#!/usr/bin/env python3
"""Smoke-test the NUCLEO-F411RE record-and-replay image on QEMU
netduinoplus2 (STM32F405) before it reaches a board.

The image records itself, sends the recording over USART2 as hex, and
scripts/hil/m3_nucleo_f411re.py (the bench script, here reading QEMU's
USART2 capture) rebuilds it and must replay it with `backstep replay` on
the nucleo-f411re memory map. Then QEMU's own instruction trace of the
recorded stretch must equal the replay's, state for state, as in
scripts/replay-test.py.

Usage: qemu-nucleo-replay-smoke.py --backstep <backstep> --elf <record_hw.elf>

SPDX-License-Identifier: MIT
"""

from __future__ import annotations

import argparse
import importlib.util
import subprocess
import sys
import tempfile
from pathlib import Path

HERE = Path(__file__).resolve().parent


def load(name: str, path: Path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)  # type: ignore[union-attr]
    return module


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--backstep", required=True)
    ap.add_argument("--elf", required=True)
    args = ap.parse_args()
    replay_test = load("replay_test", HERE / "replay-test.py")
    backstep = str(Path(args.backstep).resolve())
    elf = Path(args.elf).resolve()
    failures = 0
    with tempfile.TemporaryDirectory() as tmp:
        work = Path(tmp)
        log, usart = work / "qemu.log", work / "usart2.txt"
        subprocess.run(["timeout", "30", "qemu-system-arm", "-machine", "netduinoplus2", "-nographic",
                        "-monitor", "none", "-serial", "null", "-serial", f"file:{usart}",
                        "-accel", "tcg,one-insn-per-tb=on", "-d", "cpu,exec,nochain", "-D", str(log),
                        "-kernel", str(elf)], capture_output=True)
        rec = work / "recording.bsr"
        r = subprocess.run([sys.executable, str(HERE / "hil" / "m3_nucleo_f411re.py"), "--from-file",
                            str(usart), "--backstep", backstep, "--elf", str(elf), "--out", str(rec)],
                           capture_output=True, text=True)
        ok = r.returncode == 0
        print(f"{'PASS' if ok else 'FAIL'} bench script: recording received and replayed")
        if not ok:
            print(r.stdout + r.stderr)
            failures += 1
        else:
            trace = work / "replay.trace"
            subprocess.run([backstep, "replay", str(elf), str(rec), "--trace", str(trace)],
                           check=True, capture_output=True)
            stop_pc = replay_test.nm(elf)["bs_snapshot_take"] & ~1
            diff = replay_test.compare_traces(log, trace, stop_pc)
            states = len(replay_test.difftest.parse(trace.read_text()))
            print(f"{'PASS' if diff is None else 'FAIL'} {states} replayed states identical to QEMU's "
                  f"execution" + (f": {diff}" if diff else ""))
            failures += diff is not None
    print(f"qemu-nucleo-replay-smoke: {'PASS' if failures == 0 else 'FAIL'}")
    return 0 if failures == 0 else 1


if __name__ == "__main__":
    sys.exit(main())
