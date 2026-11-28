#!/usr/bin/env python3
"""Smoke-test the NUCLEO-F411RE interrupt record-and-replay image on QEMU
netduinoplus2 (STM32F405) before it reaches a board.

The image records 20 interrupt-driven runs and a crash run and sends each
recording over USART2 as hex. scripts/hil/m4_nucleo_f411re.py (the bench
script, here reading QEMU's USART2 capture) rebuilds and replays every one
with `backstep replay` on the nucleo-f411re memory map; all must replay
exactly, and the crash run must end in a HardFault and replay too.

QEMU runs with -icount so the run is reproducible. Its STM32 timer model
fires TIM3 and TIM4 mostly in the first run (see record_irq_hw.c), so the
three sources are required over all runs here, and in every run on the
board.

Usage: qemu-nucleo-irq-smoke.py --backstep <backstep> --elf <record_irq_hw.elf>

SPDX-License-Identifier: MIT
"""

from __future__ import annotations

import argparse
import subprocess
import sys
import tempfile
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--backstep", required=True)
    ap.add_argument("--elf", required=True)
    ap.add_argument("--timeout", type=float, default=300.0)
    args = ap.parse_args()
    backstep = str(Path(args.backstep).resolve())
    elf = Path(args.elf).resolve()
    with tempfile.TemporaryDirectory() as tmp:
        usart = Path(tmp) / "usart2.txt"
        qemu = subprocess.Popen(["qemu-system-arm", "-machine", "netduinoplus2", "-nographic",
                                 "-monitor", "none", "-serial", "null", "-serial", f"file:{usart}",
                                 "-icount", "shift=0", "-kernel", str(elf)],
                                stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        # The Nucleo image has no way to exit QEMU: stop it once it is done.
        deadline = time.monotonic() + args.timeout
        try:
            while time.monotonic() < deadline:
                if usart.exists() and b"RESULT " in usart.read_bytes():
                    break
                time.sleep(0.5)
        finally:
            qemu.terminate()
            qemu.wait()
        r = subprocess.run([sys.executable, str(HERE / "hil" / "m4_nucleo_f411re.py"), "--from-file",
                            str(usart), "--backstep", backstep, "--elf", str(elf),
                            "--out-dir", tmp, "--sources-overall"], capture_output=True, text=True)
        print(r.stdout.strip())
        ok = r.returncode == 0
        if not ok:
            print(r.stderr.strip())
    print(f"qemu-nucleo-irq-smoke: {'PASS' if ok else 'FAIL'}")
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
