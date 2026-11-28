#!/usr/bin/env python3
"""M4 acceptance test: record interrupt-driven programs on QEMU, replay
them, and check the replay against QEMU's own execution.

firmware/.../replay_irq_test.elf has three interrupt sources at three
priorities (SysTick 0x40, TIMER0 0x80, TIMER1 0xC0) whose handlers pick
random reload values, so interrupts nest and land at random points of the
main code (checksums, a sort, recursion, peripheral polling, flag waits).

  1. Runs: seeds 1..N (default 100), 400 interrupts each, on QEMU with
     -icount. Each recording must replay exactly: every peripheral load and
     every interrupt at its recorded position, and the end registers and RAM
     checksum. Over all runs, all three sources and nested interrupts must
     have been seen.
  2. Crash runs: the timer 1 handler corrupts a function pointer, the main
     code faults, and the HardFault ends the recording. These replay too.
     Ambiguous runs: the main code also spins in a loop whose iterations
     the interrupt key cannot tell apart (only a counter in RAM differs);
     replay must find the right iteration by going back (lookahead), and
     report that it did.
  3. Traces: a few shorter runs with an instruction trace (-d cpu,exec);
     QEMU's states from the start of the recording to its end must equal the
     replay's, state for state (r0-r15, xPSR), handlers included. Left out
     on both sides: the capture code (MPU-trap handler and its vectors, instruction decoder,
     bs_irq_record, bs_stack_hash), which records on the chip and returns at
     once in replay. Inside the interrupt stubs, r0-r3, r12, LR and the flags
     are what bs_irq_record left; the stub zeroes them (and reloads LR)
     before the handler runs, so they are masked there.
  4. Corruptions of the IRQS chunk: byte flips are rejected by the
     checksums; re-signed edits (an interrupt's position, number, registers,
     stack hash, one event missing or added, dropped events) are caught.

Usage: replay-irq-test.py --backstep <backstep> --elf <replay_irq_test.elf>
       [--runs N] [--traces N] [--crashes N]

SPDX-License-Identifier: MIT
"""

from __future__ import annotations

import argparse
import importlib.util
import random
import re
import struct
import subprocess
import sys
import tempfile
from collections import Counter
from pathlib import Path

HERE = Path(__file__).resolve().parent


def _load(name: str, file: str):
    spec = importlib.util.spec_from_file_location(name, HERE / file)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)  # type: ignore[union-attr]
    return module


difftest = _load("emu_difftest", "emu-difftest.py")
m3 = _load("replay_test", "replay-test.py")

EVENT_WORDS = 20  # struct bs_irq_event
EVENT_BYTES = 4 * EVENT_WORDS
IRQS_HEADER = 12  # count, dropped, stack top
CAPTURE_FILES = ("mpu_trap.c", "thumb_ldst.c", "irq_record.c")
CAPTURE_VECTORS = ("MemManage_Handler", "HardFault_Handler")  # branches to mpu_trap.c
# SysTick_Handler only branches to its stub; an interrupt tail-chained from
# the capture handler enters it with the capture's r0-r3 and flags.
STUBS = ("SysTick_Handler", "systick_stub", "timer0_stub", "timer1_stub")
SCRATCH = (0, 1, 2, 3, 12, 14)
FLAGS = 0xF80F0000  # NZCVQ and GE


def symbols_with_sizes(elf: Path) -> tuple[list[tuple[int, int]], list[tuple[int, int]]]:
    """(capture code ranges, interrupt stub ranges)."""
    out = subprocess.run(["arm-none-eabi-nm", "-l", "-S", "--defined-only", str(elf)],
                         capture_output=True, text=True, check=True).stdout
    capture, stubs = [], []
    for line in out.splitlines():
        p = line.split()
        if len(p) < 4 or p[2].lower() != "t":
            continue
        start, size = int(p[0], 16), int(p[1], 16)
        if p[3] in STUBS:
            stubs.append((start, start + size))
        elif p[3] in CAPTURE_VECTORS or (len(p) > 4 and any(f"/{f}:" in p[4] for f in CAPTURE_FILES)):
            capture.append((start, start + size))
    return capture, stubs


def inside(pc: int, ranges: list[tuple[int, int]]) -> bool:
    return any(a <= pc < b for a, b in ranges)


def qemu(elf: Path, arg: str, work: Path, trace: Path | None = None) -> str:
    cmd = ["timeout", "120", "qemu-system-arm", "-machine", "mps2-an386", "-cpu", "cortex-m4",
           "-nographic", "-monitor", "none", "-serial", "stdio",
           "-semihosting-config", f"enable=on,target=native,arg={arg}", "-icount", "shift=0"]
    if trace is not None:
        cmd += ["-accel", "tcg,one-insn-per-tb=on", "-d", "cpu,exec,nochain", "-D", str(trace)]
    r = subprocess.run(cmd + ["-kernel", str(elf)], cwd=work, capture_output=True, text=True)
    return r.stdout.replace("\r", "")


def events(data: bytes) -> list[tuple[int, ...]]:
    irqs = next(p for k, p in m3.chunks(data) if k == b"IRQS")
    count = struct.unpack_from("<I", irqs, 0)[0]
    return [struct.unpack_from(f"<{EVENT_WORDS}I", irqs, IRQS_HEADER + EVENT_BYTES * i)
            for i in range(count)]


def filtered(states, capture, stubs) -> list[tuple[int, ...]]:
    out: list[tuple[int, ...]] = []
    for s in states:
        if inside(s[15], capture):
            continue
        if inside(s[15], stubs):
            s = tuple(0 if i in SCRATCH else (v & ~FLAGS if i == 16 else v) for i, v in enumerate(s))
        if out and out[-1] == s:
            continue  # a trapped load is logged again after the capture handler
        out.append(s)
    return out


def compare_traces(qemu_log: Path, replay_trace: Path, stop_pc: int, capture,
                   stubs) -> tuple[str | None, int]:
    """(None if QEMU's execution of the recorded stretch equals the replay,
    else the first difference; states compared)."""
    replayed = filtered(difftest.parse(replay_trace.read_text()), capture, stubs)
    states = difftest.parse(qemu_log.read_text())
    start_pc = replayed[0][15]
    start = next((i for i, s in enumerate(states) if s[15] == start_pc and s[16] & 0x1FF == 0), None)
    if start is None:
        return f"QEMU never reached the start pc {start_pc:#x}", 0
    stop = next((i for i, s in enumerate(states) if i > start and s[15] == stop_pc), len(states) - 1)
    recorded = filtered(states[start:stop + 1], capture, stubs)
    if recorded == replayed:
        return None, len(replayed)
    for i, (a, b) in enumerate(zip(recorded, replayed)):
        if a != b:
            diffs = [f"{n}: qemu {x:08x} replay {y:08x}" for n, x, y in zip(difftest.names(), a, b) if x != y]
            return f"state {i} (pc {a[15]:#x}) differs: " + "; ".join(diffs), i
    return f"{len(recorded)} QEMU states vs {len(replayed)} replayed", 0


def irq_corruptions(data: bytes) -> list[tuple[str, bytes, int]]:
    """(description, re-signed recording, expected exit code)."""
    out = []
    count = len(events(data))

    def variant(name: str, expected: int, edit) -> None:
        parts = [(k, bytearray(p)) for k, p in m3.chunks(data)]
        edit({k.decode().strip(): p for k, p in parts})
        out.append((name, m3.sign(parts), expected))

    def field(c, index: int, word_no: int, change) -> None:
        m3.word(c["IRQS"], IRQS_HEADER + EVENT_BYTES * index + 4 * word_no, change)

    for index, where in ((0, "first"), (count // 2, "middle"), (count - 1, "last")):
        variant(f"pc of the {where} interrupt", 1, lambda c, i=index: field(c, i, 16, lambda v: v + 2))
        variant(f"r5 of the {where} interrupt", 1, lambda c, i=index: field(c, i, 6, lambda v: v ^ 1))
        variant(f"peripheral-load index of the {where} interrupt", 1,
                lambda c, i=index: field(c, i, 18, lambda v: v + 1))
        variant(f"stack hash of the {where} interrupt", 1,
                lambda c, i=index: field(c, i, 19, lambda v: v ^ 0x100))
        variant(f"exception number of the {where} interrupt", 1,
                lambda c, i=index: field(c, i, 0, lambda v: 24 if v != 24 else 25))
    variant("one interrupt missing", 1,
            lambda c: (m3.word(c["IRQS"], 0, lambda v: v - 1), c["IRQS"].__delitem__(slice(-EVENT_BYTES, None))))
    variant("one interrupt too many", 1,
            lambda c: (m3.word(c["IRQS"], 0, lambda v: v + 1), c["IRQS"].extend(c["IRQS"][-EVENT_BYTES:])))
    variant("the interrupt stack top", 1, lambda c: m3.word(c["IRQS"], 8, lambda v: v - 64))
    variant("interrupts dropped", 2, lambda c: m3.word(c["IRQS"], 4, lambda v: 1))
    return out


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--backstep", required=True)
    ap.add_argument("--elf", required=True)
    ap.add_argument("--runs", type=int, default=100)
    ap.add_argument("--crashes", type=int, default=10)
    ap.add_argument("--traces", type=int, default=4)
    ap.add_argument("--ambiguous", type=int, default=20)
    args = ap.parse_args()
    backstep = str(Path(args.backstep).resolve())
    elf = Path(args.elf).resolve()
    symbols = m3.nm(elf)
    stop_pc = symbols["bs_snapshot_take"] & ~1
    capture, stubs = symbols_with_sizes(elf)
    failures = 0

    def check(ok: bool, name: str, detail: str = "") -> None:
        nonlocal failures
        print(f"{'PASS' if ok else 'FAIL'} {name}" + (f": {detail}" if detail and not ok else ""))
        failures += not ok

    def replay(rec: Path, trace: Path | None = None) -> tuple[int, str]:
        cmd = [backstep, "replay", str(elf), str(rec)] + (["--trace", str(trace)] if trace else [])
        r = subprocess.run(cmd, capture_output=True, text=True)
        return r.returncode, (r.stdout.splitlines()[0] if r.returncode == 0 else r.stderr.strip())

    with tempfile.TemporaryDirectory() as tmp:
        sources: Counter[int] = Counter()
        nested = total = instructions = passed = 0
        sample = b""
        for s in range(1, args.runs + 1):
            work = Path(tmp) / f"s{s}"
            work.mkdir()
            serial = qemu(elf, f"s={s}", work)
            rec = work / "recording.bsr"
            if "RESULT PASS" not in serial or not rec.exists():
                check(False, f"seed {s}: recorded on QEMU", serial.strip()[-300:])
                continue
            data = rec.read_bytes()
            code, summary = replay(rec)
            if code != 0:
                check(False, f"seed {s}: replay", summary)
                continue
            passed += 1
            evs = events(data)
            sources.update(e[0] for e in evs)
            nested += sum(e[17] & 0x1FF != 0 for e in evs)
            total += len(evs)
            instructions += int(re.search(r"(\d+) instructions", summary).group(1))
            if s == 3:
                sample = data
            rec.unlink()
        check(passed == args.runs, f"{passed} of {args.runs} interrupt recordings replay exactly "
              f"({total} interrupts, {nested} of them nested, {instructions} instructions)")
        check(set(sources) == {15, 24, 25} and nested > 0,
              "all three sources and nested interrupts seen",
              f"sources {dict(sources)}, nested {nested}")

        crashed = 0
        for s in range(1, args.crashes + 1):
            work = Path(tmp) / f"c{s}"
            work.mkdir()
            serial = qemu(elf, f"c={s}", work)
            rec = work / "recording.bsr"
            if "crash at 0x00001000" not in serial or not rec.exists():
                check(False, f"crash seed {s}: the main code faulted and was recorded", serial.strip()[-300:])
                continue
            code, summary = replay(rec)
            if code != 0:
                check(False, f"crash seed {s}: replay", summary)
                continue
            crashed += 1
        check(crashed == args.crashes, f"{crashed} of {args.crashes} crash recordings (ended by the "
              "HardFault) replay exactly")

        found, retried = 0, 0
        for s in range(1, args.ambiguous + 1):
            work = Path(tmp) / f"a{s}"
            work.mkdir()
            serial = qemu(elf, f"a={s}", work)
            rec = work / "recording.bsr"
            r = subprocess.run([backstep, "replay", str(elf), str(rec)], capture_output=True, text=True)
            if "RESULT PASS" not in serial or r.returncode != 0:
                check(False, f"ambiguous seed {s}: replay", (r.stderr or serial).strip()[-300:])
                continue
            found += 1
            m = re.search(r"(\d+) interrupt placements retried", r.stdout)
            retried += int(m.group(1)) if m else 0
        check(found == args.ambiguous and (retried > 0 or args.ambiguous == 0),
              f"{found} of {args.ambiguous} recordings with ambiguous interrupt keys replay exactly "
              f"({retried} placements retried)")

        for s, arg in [(s, f"s={s}:n=24") for s in range(1, args.traces + 1)] + [(1, "c=1:n=24"),
                                                                                (1, "a=1:n=24")]:
            work = Path(tmp) / f"t{s}{arg[0]}"
            work.mkdir()
            log = work / "qemu.log"
            qemu(elf, arg, work, log)
            rec = work / "recording.bsr"
            trace = work / "replay.trace"
            code, summary = replay(rec, trace)
            if code != 0:
                check(False, f"trace run {arg}: replay", summary)
                continue
            diff, compared = compare_traces(log, trace, stop_pc, capture, stubs)
            in_handlers = sum(s[16] & 0x1FF != 0 for s in difftest.parse(trace.read_text()))
            check(diff is None, f"trace run {arg}: {compared} replayed states identical to QEMU's "
                  f"({len(events(rec.read_bytes()))} interrupts, {in_handlers} states in handlers)",
                  diff or "")
            log.unlink()
            trace.unlink()

        if sample:
            irqs_start = sample.index(b"IRQS") + 8
            irqs_len = struct.unpack_from("<I", sample, irqs_start - 4)[0]
            rnd = random.Random(4)
            positions = rnd.sample(range(irqs_start, irqs_start + irqs_len + 4), 200)
            rejected = 0
            path = Path(tmp) / "flipped.bsr"
            for pos in positions:
                bad = bytearray(sample)
                bad[pos] ^= 1 << rnd.randrange(8)
                path.write_bytes(bad)
                rejected += subprocess.run([backstep, "inspect", str(path)], capture_output=True).returncode == 2
            check(rejected == len(positions), f"{rejected} of {len(positions)} byte flips in IRQS rejected")
            cases = misses = 0
            path = Path(tmp) / "corrupt.bsr"
            for name, bad, expected in irq_corruptions(sample):
                path.write_bytes(bad)
                code, detail = replay(path)
                cases += 1
                if code != expected:
                    misses += 1
                    check(False, f"corrupted {name}", f"exit {code}, expected {expected}: {detail}")
            check(misses == 0, f"{cases} re-signed IRQS corruptions detected by replay")

    print(f"replay-irq-test: {'PASS' if failures == 0 else 'FAIL'}")
    return 0 if failures == 0 else 1


if __name__ == "__main__":
    sys.exit(main())
