#!/usr/bin/env python3
"""M3 hardware-in-the-loop test: record on NUCLEO-F411RE, replay on the PC.

Runs firmware/build/nucleo-f411re/record_hw.bin, which records polling
workloads that read real STM32F411 registers (TIM2, GPIO, USART, RCC) and
sends the recording over the ST-LINK virtual COM port as hex. This script
rebuilds recording.bsr from it and runs `backstep replay` on the ELF:
the replay's end registers and RAM checksum must equal the ones the board
recorded, and every peripheral load must be reproduced.

Steps
  1. Build:  make -C firmware ; (cd host && cargo build --release)
  2. Flash:  copy firmware/build/nucleo-f411re/record_hw.bin onto the
             NODE_F411RE USB drive (or use probe-rs download).
  3. Run:    python3 scripts/hil/m3_nucleo_f411re.py <serial-port>
  4. Press the black RESET button when asked; paste the report block.

`--from-file <capture>` reads a saved console capture instead of a port
(used by scripts/qemu-nucleo-replay-smoke.sh with QEMU's netduinoplus2).

SPDX-License-Identifier: MIT
"""

from __future__ import annotations

import argparse
import subprocess
import sys
import time
import zlib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
BANNER = "BACKSTEP-M3 NUCLEO-F411RE record and replay test"


def read_port(port_name: str, timeout: float) -> list[str]:
    try:
        import serial  # type: ignore[import-not-found]
    except ImportError:
        print("pyserial is required: pip install pyserial", file=sys.stderr)
        sys.exit(2)
    lines: list[str] = []
    with serial.Serial(port_name, 115200, timeout=0.2) as port:
        port.reset_input_buffer()
        print(">>> Press the black RESET button on the Nucleo now.")
        deadline = time.monotonic() + timeout
        buffer = b""
        while time.monotonic() < deadline:
            buffer += port.read(4096)
            while b"\n" in buffer:
                raw, buffer = buffer.split(b"\n", 1)
                line = raw.decode("ascii", errors="replace").strip()
                if line == BANNER:
                    lines.clear()
                lines.append(line)
                if line.startswith("RESULT "):
                    return lines
    return lines


def extract(lines: list[str]) -> tuple[bytes | None, str]:
    """The recording from the console lines, or None and why not."""
    try:
        begin = next(i for i, l in enumerate(lines) if l.startswith("BSREC BEGIN "))
        end = next(i for i, l in enumerate(lines) if l.startswith("BSREC END "))
    except StopIteration:
        return None, "no BSREC BEGIN/END block"
    size = int(lines[begin].split()[2])
    crc = int(lines[end].split()[2], 16)
    try:
        data = bytes.fromhex("".join(lines[begin + 1:end]))
    except ValueError:
        return None, "the hex block is damaged"
    if len(data) != size:
        return None, f"{len(data)} bytes received, {size} sent"
    if zlib.crc32(data) != crc:
        return None, "CRC-32 of the received bytes does not match (serial error)"
    return data, f"{size} bytes, CRC-32 {crc:08X}"


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("port", nargs="?", help="serial port of the Nucleo's ST-LINK VCP")
    parser.add_argument("--from-file", help="read a saved console capture instead of a port")
    parser.add_argument("--backstep", default=str(ROOT / "host/target/release/backstep"))
    parser.add_argument("--elf", default=str(ROOT / "firmware/build/nucleo-f411re/record_hw.elf"))
    parser.add_argument("--out", default="recording.bsr", help="where to save the recording")
    parser.add_argument("--timeout", type=float, default=30.0)
    args = parser.parse_args()
    if args.from_file:
        lines = [l.strip() for l in Path(args.from_file).read_text(errors="replace").splitlines()]
    elif args.port:
        lines = read_port(args.port, args.timeout)
    else:
        parser.error("give a serial port or --from-file")

    results: list[tuple[str, bool, str]] = []
    results.append(("test banner received", BANNER in lines, ""))
    results.append(("board finished the recording", "PASS recording complete" in lines, ""))
    data, detail = extract(lines)
    results.append(("recording received intact", data is not None, detail))
    replay_out = ""
    if data is not None:
        Path(args.out).write_bytes(data)
        r = subprocess.run([args.backstep, "replay", args.elf, args.out],
                           capture_output=True, text=True)
        replay_out = (r.stdout + r.stderr).strip()
        first = replay_out.splitlines()[0] if replay_out else f"exit {r.returncode}"
        results.append(("backstep replay reproduces the run (end registers, RAM CRC, every load)",
                        r.returncode == 0, first))
    else:
        results.append(("backstep replay reproduces the run (end registers, RAM CRC, every load)",
                        False, "no recording"))
    results.append(("firmware verdict RESULT PASS", "RESULT PASS" in lines, ""))

    ok = all(passed for _, passed, _ in results)
    print("\n----- copy from here -----")
    print("### M3 hardware test: record on NUCLEO-F411RE, replay on the PC")
    for name, passed, d in results:
        print(f"- [{'x' if passed else ' '}] {name}" + (f" ({d})" if d else ""))
    if replay_out:
        print("\n```\n" + replay_out + "\n```")
    print(f"\n**Result: {'PASS' if ok else 'FAIL'}**")
    print("----- to here -----")
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
