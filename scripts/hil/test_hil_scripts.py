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
M1 = HERE / "m1_nucleo_f411re.py"
M2 = HERE / "m2_nucleo_f411re.py"
M3 = HERE / "m3_nucleo_f411re.py"


def fake_board(master_fd: int, lines: list[str], period_s: float) -> None:
    time.sleep(0.5)  # give the script time to open the port
    for line in lines:
        os.write(master_fd, (line + "\r\n").encode())
        time.sleep(period_s)


def run_case(name: str, lines: list[str], period_s: float, led: str, expect_ok: bool,
             script: Path = M0, extra: list[str] | None = None) -> bool:
    master, slave = pty.openpty()
    port = os.ttyname(slave)
    feeder = threading.Thread(target=fake_board, args=(master, lines, period_s), daemon=True)
    feeder.start()
    proc = subprocess.run(
        [sys.executable, str(script), port, "--timeout", "15", *(extra or [])],
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

    m1_good = [
        "BACKSTEP-M1 NUCLEO-F411RE MPU trap test",
        "PASS GPIOB_AFRH holds test pattern",
        "PASS ldr.n imm5",
        "PASS   ...logged once with address and size",
        "MEASURE cycles_per_1000_loads trapped=260000 plain=5000 "
        "extra_cycles_per_trapped_load=255",
        "PASS DWT cycle counter works",
        "RESULT PASS",
    ]
    m1_fail = m1_good[:2] + ["FAIL ldr.n imm5", "     got 0x0 want 0x8A6B4C2D"] + m1_good[3:-1] + ["RESULT FAIL"]
    m1_absurd = [l.replace("extra_cycles_per_trapped_load=255",
                           "extra_cycles_per_trapped_load=9") for l in m1_good]
    m1_cases = [
        ("m1: healthy board passes", m1_good, 0.05, "", True),
        ("m1: one failing check fails", m1_fail, 0.05, "", False),
        ("m1: implausible cycle count fails", m1_absurd, 0.05, "", False),
        ("m1: no RESULT line fails", m1_good[:-1], 0.05, "", False),
        ("m1: silent board fails", [], 0.05, "", False),
    ]
    results += [run_case(*case, script=M1) for case in m1_cases]

    m2_good = [
        "BACKSTEP-M2 NUCLEO-F411RE exclusive monitor test",
        "PASS ldrex A; strex A succeeds",
        "PASS   ...and stores",
        "PASS ldrex A; clrex; strex A fails",
        "PASS   ...and does not store",
        "MEASURE excl other_address status=0",
        "MEASURE excl other_size status=0",
        "MEASURE excl plain_store status=0",
        "PASS PendSV taken between ldrex and strex",
        "MEASURE excl exception status=1",
        "RESULT PASS",
    ]
    m2_fail = [l.replace("PASS ldrex A; clrex", "FAIL ldrex A; clrex") for l in m2_good[:-1]] + ["RESULT FAIL"]
    m2_missing = [l for l in m2_good if "plain_store" not in l]
    m2_cases = [
        ("m2: healthy board passes", m2_good, 0.05, "", True),
        ("m2: failed required check fails", m2_fail, 0.05, "", False),
        ("m2: missing measurement fails", m2_missing, 0.05, "", False),
        ("m2: silent board fails", [], 0.05, "", False),
    ]
    results += [run_case(*case, script=M2) for case in m2_cases]

    # M3: the board sends a recording as hex; a stub stands in for
    # `backstep replay` (its exit status is the replay verdict).
    import tempfile
    import zlib
    with tempfile.TemporaryDirectory() as tmp:
        stubs = {}
        for name, code in (("ok", 0), ("diverged", 1)):
            stub = Path(tmp) / f"backstep-{name}"
            stub.write_text(f"#!{sys.executable}\nimport sys\nprint('replay: stub')\nsys.exit({code})\n")
            stub.chmod(0o755)
            stubs[name] = stub
        payload = bytes(range(256)) * 3
        hexed = payload.hex().upper()
        block = [hexed[i:i + 64] for i in range(0, len(hexed), 64)]
        m3_good = ["BACKSTEP-M3 NUCLEO-F411RE record and replay test", "PASS recording complete",
                   f"BSREC BEGIN {len(payload)}", *block, f"BSREC END 0x{zlib.crc32(payload):08X}",
                   "RESULT PASS"]
        m3_damaged = [l.replace("00010203", "00010213") for l in m3_good]
        m3_cut = [l for l in m3_good if not l.startswith("BSREC END")]
        out = ["--out", str(Path(tmp) / "recording.bsr"), "--elf", "unused.elf"]
        m3_cases = [
            ("m3: healthy board and replay passes", m3_good, 0.02, "", True, stubs["ok"]),
            ("m3: replay divergence fails", m3_good, 0.02, "", False, stubs["diverged"]),
            ("m3: damaged transfer fails", m3_damaged, 0.02, "", False, stubs["ok"]),
            ("m3: missing end of block fails", m3_cut, 0.02, "", False, stubs["ok"]),
            ("m3: silent board fails", [], 0.02, "", False, stubs["ok"]),
        ]
        for name, lines, period, led, expect, stub in m3_cases:
            results.append(run_case(name, lines, period, led, expect, script=M3,
                                    extra=out + ["--backstep", str(stub)]))
    print(f"hil script self-test: {sum(results)}/{len(results)} cases correct")
    return 0 if all(results) else 1


if __name__ == "__main__":
    sys.exit(main())
