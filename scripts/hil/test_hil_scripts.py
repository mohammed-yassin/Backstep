#!/usr/bin/env python3
"""Self-test for the hardware-in-the-loop scripts, run in CI without a board.

A pseudo-terminal stands in for the Nucleo's virtual COM port and replays
the serial output real firmware would produce, so the HIL script's parsing
and verdict logic is tested before anyone relies on it at the bench.

SPDX-License-Identifier: MIT
"""

from __future__ import annotations

import os
import pty
import subprocess
import sys
import threading
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
M0 = HERE / "m0_nucleo_f411re.py"


def fake_board(master_fd: int, lines: list[str], period_s: float) -> None:
    time.sleep(0.5)  # give the script time to open the port
    for line in lines:
        os.write(master_fd, (line + "\r\n").encode())
        time.sleep(period_s)


def run_case(name: str, lines: list[str], period_s: float, led: str, expect_ok: bool) -> bool:
    master, slave = pty.openpty()
    port = os.ttyname(slave)
    feeder = threading.Thread(target=fake_board, args=(master, lines, period_s), daemon=True)
    feeder.start()
    proc = subprocess.run(
        [sys.executable, str(M0), port, "--timeout", "15"],
        input=led + "\n", capture_output=True, text=True, timeout=30,
    )
    os.close(master)
    os.close(slave)
    ok = (proc.returncode == 0) == expect_ok
    print(f"{'PASS' if ok else 'FAIL'} {name} (exit {proc.returncode})")
    if not ok:
        print(proc.stdout, proc.stderr)
    return ok


def main() -> int:
    good = ["BACKSTEP-M0 NUCLEO-F411RE blinky recorder=0.0.1 crc=CBF43926"] + [
        f"BACKSTEP-M0 alive {n}" for n in range(1, 6)
    ]
    bad_crc = ["BACKSTEP-M0 NUCLEO-F411RE blinky recorder=0.0.1 crc=DEADBEEF"] + good[1:]
    gap = good[:3] + ["BACKSTEP-M0 alive 4", "BACKSTEP-M0 alive 5", "BACKSTEP-M0 alive 6"]
    cases = [
        ("healthy board passes", good, 1.0, "y", True),
        ("LED not blinking fails", good, 1.0, "n", False),
        ("wrong CRC fails", bad_crc, 1.0, "y", False),
        ("clock 20% fast fails", good, 0.8, "y", False),
        ("missing heartbeat fails", gap, 1.0, "y", False),
        ("silent board fails", [], 1.0, "y", False),
    ]
    results = [run_case(*case) for case in cases]
    print(f"hil script self-test: {sum(results)}/{len(results)} cases correct")
    return 0 if all(results) else 1


if __name__ == "__main__":
    sys.exit(main())
