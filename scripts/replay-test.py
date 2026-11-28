#!/usr/bin/env python3
"""M3 acceptance test: record polling programs on QEMU, replay them, and
check the replay against QEMU's own execution and the recording's checks.

For each of the ten workloads of firmware/.../replay_test.elf:
  1. Run it on QEMU (mps2-an386) with an instruction trace (-d cpu,exec)
     and semihosting; it writes recording.bsr.
  2. `backstep replay` it, writing its own trace. It must pass: every
     peripheral load matches the recording, and so do the end registers
     and RAM checksum.
  3. QEMU's trace from the start of the recording to its end, without the
     states of the MPU-trap handler (which records loads on the chip and
     does not run in replay), must equal the replay's trace state for
     state: r0-r15 and xPSR before every instruction.
Then the self-check is attacked with corrupted recordings:
  4. Byte flips (every header byte, and a sample through the file) must be
     rejected by the checksums (`backstep inspect` exits with 2).
  5. Corruptions with valid checksums (re-signed), each of which changes
     the execution or the end state, must be caught by replay (exit 1, or
     2 for a different firmware or target).

Usage: replay-test.py --backstep <backstep> --elf <replay_test.elf>

SPDX-License-Identifier: MIT
"""

from __future__ import annotations

import argparse
import importlib.util
import random
import struct
import subprocess
import sys
import tempfile
import zlib
from pathlib import Path

HERE = Path(__file__).resolve().parent
_spec = importlib.util.spec_from_file_location("emu_difftest", HERE / "emu-difftest.py")
difftest = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(difftest)  # type: ignore[union-attr]

WORKLOADS = 10


def nm(elf: Path) -> dict[str, int]:
    out = subprocess.run(["arm-none-eabi-nm", str(elf)], capture_output=True, text=True, check=True)
    return {p[2]: int(p[0], 16) for p in (line.split() for line in out.stdout.splitlines()) if len(p) == 3}


def run_qemu(elf: Path, workload: int, work: Path) -> Path:
    log = work / "qemu.log"
    subprocess.run(["timeout", "60", "qemu-system-arm", "-machine", "mps2-an386", "-cpu", "cortex-m4",
                    "-nographic", "-monitor", "none", "-serial", "null",
                    "-semihosting-config", f"enable=on,target=native,arg=w={workload}",
                    "-accel", "tcg,one-insn-per-tb=on", "-d", "cpu,exec,nochain", "-D", str(log),
                    "-kernel", str(elf)], check=True, cwd=work)
    return log


def compare_traces(qemu_log: Path, replay_trace: Path, stop_pc: int) -> str | None:
    """None if QEMU's execution of the recorded stretch equals the replay."""
    replayed = difftest.parse(replay_trace.read_text())
    states = difftest.parse(qemu_log.read_text())
    start_pc = replayed[0][15]
    start = next((i for i, s in enumerate(states) if s[15] == start_pc and s[16] & 0x1FF == 0), None)
    if start is None:
        return f"QEMU never reached the start pc {start_pc:#x}"
    recorded = []
    for s in states[start:]:
        if s[16] & 0x1FF != 0:
            continue  # the MPU-trap handler (MemManage/HardFault)
        if recorded and recorded[-1] == s:
            continue  # the faulting load is logged again after the handler
        recorded.append(s)
        if s[15] == stop_pc:
            break
    if recorded == replayed:
        return None
    for i, (a, b) in enumerate(zip(recorded, replayed)):
        if a != b:
            diffs = [f"{n}: qemu {x:08x} replay {y:08x}" for n, x, y in zip(difftest.names(), a, b) if x != y]
            return f"state {i} differs: " + "; ".join(diffs)
    return f"{len(recorded)} QEMU states vs {len(replayed)} replayed"


# --- recordings, re-signed (mirrors docs/adr/0008-recording-format.md) ---

def chunks(data: bytes) -> list[tuple[bytes, bytearray]]:
    out, pos = [], 16
    for _ in range(4):
        kind = data[pos:pos + 4]
        (length,) = struct.unpack_from("<I", data, pos + 4)
        out.append((kind, bytearray(data[pos + 8:pos + 8 + length])))
        pos += 12 + length
    return out


def sign(parts: list[tuple[bytes, bytearray]]) -> bytes:
    body = bytearray(struct.pack("<4I", 0x43525342, 1, 16, 4))
    for kind, payload in parts:
        body += kind + struct.pack("<I", len(payload)) + payload + struct.pack("<I", zlib.crc32(payload))
    return bytes(body + struct.pack("<I", zlib.crc32(body)))


def resign_snapshot(snap: bytearray) -> None:
    """Recompute the snapshot's own region CRC (word 130 of its header)."""
    count = struct.unpack_from("<I", snap, 12)[0]
    lengths = [struct.unpack_from("<I", snap, 4 * (122 + 2 * i) + 4)[0] for i in range(count)]
    struct.pack_into("<I", snap, 4 * 130, zlib.crc32(bytes(snap[524:524 + sum(lengths)])))


def word(buf: bytearray, offset: int, change) -> None:
    struct.pack_into("<I", buf, offset, change(struct.unpack_from("<I", buf, offset)[0]) & 0xFFFFFFFF)


def semantic_corruptions(data: bytes, symbols: dict[str, int]) -> list[tuple[str, bytes, int]]:
    """(description, re-signed recording, expected exit code)."""
    out = []

    def variant(name: str, expected: int, edit) -> None:
        parts = [(k, bytearray(p)) for k, p in chunks(data)]
        edit({k.decode().strip(): p for k, p in parts})
        resign_snapshot(parts[1][1])
        out.append((name, sign(parts), expected))

    n_events = struct.unpack_from("<I", chunks(data)[2][1], 0)[0]
    for idx, where in ((0, "first"), (n_events // 2, "middle"), (n_events - 1, "last")):
        off = 8 + 16 * idx
        variant(f"value of the {where} peripheral load", 1, lambda c, o=off: word(c["MMIO"], o + 4, lambda v: v ^ 1))
        variant(f"address of the {where} peripheral load", 1, lambda c, o=off: word(c["MMIO"], o, lambda v: v + 4))
        variant(f"instruction of the {where} peripheral load", 1, lambda c, o=off: word(c["MMIO"], o + 8, lambda v: v + 2))
    variant("one peripheral load missing", 1, lambda c: (word(c["MMIO"], 0, lambda v: v - 1), c["MMIO"].__delitem__(slice(-16, None))))
    variant("one peripheral load too many", 1, lambda c: (word(c["MMIO"], 0, lambda v: v + 1), c["MMIO"].extend(c["MMIO"][-16:])))
    # Registers live at the start (r4-r11 and LR are reloaded from the stack
    # by bs_record_begin's epilogue, so changing them changes nothing: only
    # the checksums can catch that, and do, above).
    for reg, slot in (("sp", 13), ("pc", 15)):
        variant(f"starting {reg}", 1, lambda c, s=slot: word(c["SNAP"], 16 + 4 * s, lambda v: v + 8))
    g_hash = symbols["g_hash"]
    variant("a RAM byte of the snapshot (g_hash)", 1,
            lambda c: c["SNAP"].__setitem__(524 + g_hash - struct.unpack_from("<I", c["SNAP"], 4 * 122)[0],
                                            c["SNAP"][524 + g_hash - struct.unpack_from("<I", c["SNAP"], 4 * 122)[0]] ^ 0x40))
    variant("the end RAM checksum", 1, lambda c: word(c["END"], 4 * 23, lambda v: v ^ 1))
    for reg, slot in (("r5", 5), ("sp", 13), ("lr", 14), ("flags", 16)):
        variant(f"end {reg}", 1, lambda c, s=slot: word(c["END"], 4 * s, lambda v: v ^ 0x80000000 if s == 16 else v + 4))
    variant("the firmware checksum", 2, lambda c: word(c["TGTD"], 4, lambda v: v ^ 1))
    variant("the target", 2, lambda c: word(c["TGTD"], 0, lambda v: 99))
    return out


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--backstep", required=True)
    ap.add_argument("--elf", required=True)
    args = ap.parse_args()
    backstep = str(Path(args.backstep).resolve())
    elf = Path(args.elf).resolve()
    symbols = nm(elf)
    stop_pc = symbols["bs_snapshot_take"] & ~1
    failures = 0

    def check(ok: bool, name: str, detail: str = "") -> None:
        nonlocal failures
        print(f"{'PASS' if ok else 'FAIL'} {name}" + (f": {detail}" if detail and not ok else ""))
        failures += not ok

    with tempfile.TemporaryDirectory() as tmp:
        recordings = []
        for w in range(WORKLOADS):
            work = Path(tmp) / f"w{w}"
            work.mkdir()
            log = run_qemu(elf, w, work)
            rec = work / "recording.bsr"
            trace = work / "replay.trace"
            r = subprocess.run([backstep, "replay", str(elf), str(rec), "--trace", str(trace)],
                               capture_output=True, text=True)
            summary = r.stdout.splitlines()[0] if r.stdout else r.stderr.strip()
            check(r.returncode == 0, f"workload {w}: replay reproduces the recording ({summary})",
                  r.stderr.strip())
            if r.returncode == 0:
                diff = compare_traces(log, trace, stop_pc)
                states = len(difftest.parse(trace.read_text()))
                check(diff is None, f"workload {w}: {states} replayed states identical to QEMU's execution",
                      diff or "")
            recordings.append(rec.read_bytes())

        # Byte flips: the checksums reject every one.
        data = recordings[3]
        rnd = random.Random(1)
        positions = sorted(set(range(16)) | set(rnd.sample(range(len(data)), min(400, len(data)))))
        rejected = 0
        for pos in positions:
            bad = bytearray(data)
            bad[pos] ^= 1 << rnd.randrange(8)
            path = Path(tmp) / "flipped.bsr"
            path.write_bytes(bad)
            r = subprocess.run([backstep, "inspect", str(path)], capture_output=True, text=True)
            rejected += r.returncode == 2
        check(rejected == len(positions), f"{rejected} of {len(positions)} byte flips rejected by the checksums")
        truncated = Path(tmp) / "truncated.bsr"
        truncated.write_bytes(data[:-17])
        r = subprocess.run([backstep, "inspect", str(truncated)], capture_output=True, text=True)
        check(r.returncode == 2, "a truncated recording is rejected")

        # Re-signed corruptions: replay detects each.
        cases = 0
        for w in (3, 7):
            for name, bad, expected in semantic_corruptions(recordings[w], symbols):
                path = Path(tmp) / "corrupt.bsr"
                path.write_bytes(bad)
                r = subprocess.run([backstep, "replay", str(elf), str(path)], capture_output=True, text=True)
                cases += 1
                if r.returncode != expected:
                    check(False, f"workload {w}, corrupted {name}",
                          f"exit {r.returncode}, expected {expected}: {r.stderr.strip()}")
        check(failures == 0, f"{cases} re-signed corruptions detected by replay")

    print(f"replay-test: {'PASS' if failures == 0 else 'FAIL'}")
    return 0 if failures == 0 else 1


if __name__ == "__main__":
    sys.exit(main())
