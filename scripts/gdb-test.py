#!/usr/bin/env python3
"""M5 acceptance test: scripted GDB sessions against `backstep gdb`.

Recordings are made on QEMU (mps2-an386) from the test firmware, then
each session runs gdb-multiarch in batch mode on one of them through
`target remote | backstep gdb --stdio <elf> <recording>` and checks what
GDB shows. Where a session lands on a position, the registers GDB reads
must equal that position's state in `backstep replay --trace` (the plain
forward replay, itself equal to QEMU's own trace: replay-test.py and
replay-irq-test.py).

Recordings:
  irq       replay_irq_test.elf s=5:n=40   nested interrupts, 3 sources
  ambig     replay_irq_test.elf a=2:n=24   keys that need lookahead
  crash     replay_irq_test.elf c=1:n=24   ended by a HardFault
  poll      replay_test.elf w=3            polling only (M3)
  demo      demo_overrun.elf               an interrupt overruns a buffer

The demo session is the M5 criterion "a reverse watchpoint stops on the
exact instruction that corrupted memory": the line marked DEMO-BUG-STORE.
Two more tests use TCP: GDB with `target extended-remote` (as IDEs
connect), and the protocol spoken directly: no-ack mode, and stopping a
run with Ctrl-C.

Usage: gdb-test.py --backstep <backstep> --firmware <firmware/build/qemu-mps2-an386>

SPDX-License-Identifier: MIT
"""

from __future__ import annotations

import argparse
import importlib.util
import random
import re
import socket
import struct
import subprocess
import sys
import tempfile
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
_spec = importlib.util.spec_from_file_location("replay_test", HERE / "replay-test.py")
m3 = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(m3)  # type: ignore[union-attr]

GDB = "gdb-multiarch"
REG_NAMES = [f"r{i}" for i in range(13)] + ["sp", "lr", "pc", "xpsr"]


class Rec:
    """A recording, its firmware, its forward trace and symbols."""

    def __init__(self, name: str, elf: Path, arg: str, work: Path, backstep: str):
        self.name, self.elf = name, elf
        d = work / name
        d.mkdir()
        semi = "enable=on,target=native" + (f",arg={arg}" if arg else "")
        subprocess.run(["timeout", "120", "qemu-system-arm", "-machine", "mps2-an386", "-cpu", "cortex-m4",
                        "-nographic", "-monitor", "none", "-serial", "null", "-semihosting-config", semi,
                        "-icount", "shift=0", "-kernel", str(elf)], cwd=d, check=False)
        self.path = d / "recording.bsr"
        trace = d / "replay.trace"
        subprocess.run([backstep, "replay", str(elf), str(self.path), "--trace", str(trace)],
                       check=True, capture_output=True)
        self.states = parse_states(trace.read_text())
        self.symbols = m3.nm(elf)
        self.data = self.path.read_bytes()


def parse_states(text: str) -> list[tuple[int, ...]]:
    """Every state of a replay trace, duplicates kept (one per position)."""
    out, regs = [], []
    for line in text.splitlines():
        if line.startswith("R"):
            regs.extend(int(line[i:i + 8], 16) for i in (4, 17, 30, 43))
        elif line.startswith("XPSR="):
            out.append((*regs, int(line[5:13], 16)))
            regs = []
    return out


def gdb(backstep: str, rec: Rec, commands: list[str], timeout: float = 120) -> str:
    args = [GDB, "-batch", "-nx", "-ex", "set pagination off", "-ex", "set confirm off",
            "-ex", "set width 0", "-ex", f"file {rec.elf}",
            "-ex", f"target remote | {backstep} gdb --stdio {rec.elf} {rec.path}"]
    for c in commands:
        args += ["-ex", c]
    r = subprocess.run(args, capture_output=True, text=True, timeout=timeout)
    return r.stdout + r.stderr


def registers(out: str, marker: str) -> list[int] | None:
    """r0-r12, sp, lr, pc, xpsr from the `info registers` after `marker`."""
    part = out.split(marker, 1)[1] if marker in out else ""
    values = {}
    for line in part.splitlines():
        m = re.match(r"^(r\d+|sp|lr|pc|xpsr)\s+0x([0-9a-f]+)", line)
        if m and m.group(1) not in values:
            values[m.group(1)] = int(m.group(2), 16)
    if len(values) < len(REG_NAMES):
        return None
    return [values[n] for n in REG_NAMES]


def position(out: str, nth: int = -1) -> int | None:
    found = re.findall(r"position (\d+) of \d+", out)
    return int(found[nth]) if found else None


def at_line_start(out: str, p_a: int | None, p_b: int | None, p_c: int | None) -> bool:
    """Moved forwards, then backwards to no later than where it started
    from, and stopped where a source line begins, in deep() (GDB decides
    where lines begin; on optimised code a loop's line can begin twice)."""
    tail = out.split("@@")[-1]
    start = re.search(r"starts at address 0x([0-9a-f]+) <deep", tail)
    pc = re.search(r"=> 0x([0-9a-f]+) <deep", tail)
    return (None not in (p_a, p_b, p_c) and p_b > p_a and p_c < p_b and start is not None and pc is not None
            and start.group(1) == pc.group(1))


def diff_state(got: list[int] | None, want: tuple[int, ...]) -> str:
    if got is None:
        return "no registers in GDB's output"
    bad = [f"{n}: gdb {g:#x} trace {w:#x}" for n, g, w in zip(REG_NAMES, got, want) if g != w]
    return "; ".join(bad)


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--backstep", required=True)
    ap.add_argument("--firmware", required=True, help="firmware/build/qemu-mps2-an386")
    args = ap.parse_args()
    backstep = str(Path(args.backstep).resolve())
    fw = Path(args.firmware).resolve()
    results: list[bool] = []

    def check(ok: bool, name: str, detail: str = "", out: str = "") -> None:
        print(f"{'PASS' if ok else 'FAIL'} {name}" + (f": {detail}" if detail and not ok else ""))
        if not ok and out:
            print("    " + "\n    ".join(out.strip().splitlines()[-25:]))
        results.append(ok)

    with tempfile.TemporaryDirectory() as tmp:
        work = Path(tmp)
        irq_elf = fw / "replay_irq_test.elf"
        r = {
            "irq": Rec("irq", irq_elf, "s=5:n=40", work, backstep),
            "ambig": Rec("ambig", irq_elf, "a=2:n=24", work, backstep),
            "crash": Rec("crash", irq_elf, "c=1:n=24", work, backstep),
            "poll": Rec("poll", fw / "replay_test.elf", "w=3", work, backstep),
            "demo": Rec("demo", fw / "demo_overrun.elf", "", work, backstep),
        }
        irq = r["irq"]
        end = len(irq.states) - 1
        flush = "maintenance flush register-cache"
        snap_take = irq.symbols["bs_snapshot_take"] & ~1

        # 1-3: the start, steps forwards, and steps back.
        out = gdb(backstep, irq, ["echo @@\\n", "info registers"])
        check(diff_state(registers(out, "@@"), irq.states[0]) == "", "1 the state at the start equals the trace",
              diff_state(registers(out, "@@"), irq.states[0]), out)
        out = gdb(backstep, irq, ["stepi 1000", "echo @@\\n", "info registers"])
        check(diff_state(registers(out, "@@"), irq.states[1000]) == "", "2 stepi 1000 equals the trace",
              diff_state(registers(out, "@@"), irq.states[1000]), out)
        out = gdb(backstep, irq, ["stepi 1000", "reverse-stepi 1000", "echo @@\\n", "info registers"])
        check(diff_state(registers(out, "@@"), irq.states[0]) == "",
              "3 stepi 1000, reverse-stepi 1000: back at the start", diff_state(registers(out, "@@"), irq.states[0]),
              out)

        # 4-8: jumps to positions, in every recording kind.
        rnd = random.Random(5)
        for n, (rec, p) in enumerate([(irq, rnd.randrange(end)), (irq, end - 1), (r["ambig"], None),
                                      (r["crash"], None), (r["poll"], None)], start=4):
            if p is None:
                p = rnd.randrange(len(rec.states) - 1)
            out = gdb(backstep, rec, [f"monitor goto {p}", flush, "echo @@\\n", "info registers",
                                      "monitor position"])
            d = diff_state(registers(out, "@@"), rec.states[p])
            check(d == "" and position(out) == p, f"{n} monitor goto {p} ({rec.name}) equals the trace", d, out)

        # 9-10: the ends of the recording.
        out = gdb(backstep, irq, ["continue", "echo @@\\n", "info registers", "monitor position"])
        regs = registers(out, "@@")
        check("No more reverse-execution history" in out and regs is not None and regs[15] == snap_take
              and position(out) == end, "9 continue runs to the end (bs_snapshot_take)", "", out)
        out = gdb(backstep, irq, ["continue", "reverse-continue", "monitor position"])
        check(out.count("No more reverse-execution history") == 2 and position(out) == 0,
              "10 reverse-continue from the end returns to the start", "", out)

        # 11-13: breakpoints.
        isr = irq.symbols["timer0_isr"] & ~1
        hits = [p for p, s in enumerate(irq.states) if s[15] == isr]
        out = gdb(backstep, irq, ["break timer0_isr", "continue", "continue", "continue", "monitor position"])
        check(len(hits) > 3 and position(out) == hits[2], "11 a breakpoint stops at its third hit",
              f"position {position(out)}, hits {hits[:4]}", out)
        out = gdb(backstep, irq, ["monitor goto end", flush, "break timer0_isr", "reverse-continue",
                                  "monitor position", "reverse-continue", "monitor position"])
        check(position(out, -2) == hits[-1] and position(out, -1) == hits[-2],
              "12 reverse-continue stops at the last hits, latest first", f"{re.findall(r'position (\d+) of', out)}",
              out)
        out = gdb(backstep, irq, ["tbreak timer1_isr", "continue", "continue", "monitor position"])
        check("Temporary breakpoint 1, timer1_isr" in out and position(out) == end,
              "13 tbreak stops once", "", out)

        # 14-16: watchpoints, forwards (stop after the access).
        amb = r["ambig"]
        out = gdb(backstep, amb, ["watch g_spin_count"] + ["continue", "x/i $pc-2"] * 3)
        stores = re.findall(r"0x[0-9a-f]+ <[^>]*>:\s+(\w+)", out.split("Hardware watchpoint 1: g_spin_count", 1)[-1])
        check(out.count("New value") == 3 and len(stores) == 3 and all(s.startswith("str") for s in stores),
              "14 watch: each stop follows a store that changed the variable", f"{stores}", out)
        out = gdb(backstep, amb, ["rwatch g_spin_stop", "continue", "x/i $pc-2"])
        loads = re.findall(r"0x[0-9a-f]+ <[^>]*>:\s+(\w+)", out.split("Hardware read watchpoint", 1)[-1])
        check("Value = " in out and loads and loads[0].startswith("ldr"),
              "15 rwatch: stops after a load of the variable", f"{loads}", out)
        out = gdb(backstep, amb, ["awatch g_spin_stop", "continue", "continue", "monitor position"])
        check(out.count("Hardware access (read/write) watchpoint") >= 2 and (position(out) or 0) > 0,
              "16 awatch: stops on reads and writes", "", out)

        # 17-19: the demo bug, found backwards from the failure.
        demo = r["demo"]
        src = (HERE.parent / "firmware/targets/qemu-mps2-an386/demo_overrun.c").read_text().splitlines()
        bug_line = next(i + 1 for i, l in enumerate(src) if "DEMO-BUG-STORE" in l)
        out = gdb(backstep, demo, ["continue", "watch g_rx.limit", "reverse-continue", "echo @@\\n",
                                   "x/i $pc", "info line *$pc", "bt", "print g_rx_len", "print/d g_rx.limit",
                                   "stepi", "print/d g_rx.limit"])
        after = out.split("@@", 1)[-1]
        check(f"demo_overrun.c:{bug_line}" in out and re.search(r"=> 0x[0-9a-f]+ <rx_isr\+\d+>:\s+strb", after)
              is not None, f"17 demo: a reverse watchpoint stops on the store that corrupted memory (line {bug_line})",
              "", out)
        check("<signal handler called>" in after and "take_line" in after and "rx_isr" in after,
              "18 demo: the backtrace shows the interrupt and what it interrupted", "", out)
        vals = re.findall(r"\$\d+ = (\d+)", after)
        check(len(vals) == 3 and vals[0] == "17" and vals[1] == "100" and vals[2] != "100",
              "19 demo: the setting is intact before the store and corrupted after it", f"{vals}", out)

        # 20-21: a crash.
        crash = r["crash"]
        out = gdb(backstep, crash, ["continue", "bt"])
        check("bs_mmio_trap_fatal (pc=4096, reason=3)" in out and "bs_hardfault_handler" in out,
              "20 crash: the recording ends in the fault handler, at the bad call target", "", out)
        out = gdb(backstep, crash, ["continue", "break HardFault_Handler", "reverse-continue", "reverse-stepi",
                                    "echo @@\\n", "info registers"])
        regs = registers(out, "@@")
        check(regs is not None and regs[15] == 0x1000 and "HardFault_Handler" in out,
              "21 crash: back from the fault handler to the bad call target (0x1000)",
              f"pc {regs[15]:#x}" if regs else "", out)

        # 22-27: source-level stepping, forwards and backwards.
        # deep() is recursive; round_of_work (inlined into main) calls it.
        src_irq = (HERE.parent / "firmware/targets/qemu-mps2-an386/replay_irq_test.c").read_text().splitlines()
        body = next(i + 1 for i, l in enumerate(src_irq) if "acc = acc * 7u + local[i];" in l)
        out = gdb(backstep, irq, ["break deep", "continue", "monitor position", "delete", "finish",
                                  "monitor position", "bt 1"])
        a_, b_ = position(out, -2), position(out, -1)
        check(a_ is not None and b_ is not None and b_ > a_ and "Value returned" in out
              and irq.states[b_][15] == irq.states[a_][14] & ~1 and "round_of_work" in out.split("Value")[-1],
              "22 finish returns from a function to its caller's return address", f"{a_} -> {b_}", out)
        out = gdb(backstep, irq, ["break deep", "continue", "monitor position", "delete", "stepi 3",
                                  "reverse-finish", "monitor position", "bt 1"])
        a_, b_ = position(out, -2), position(out, -1)
        check(a_ is not None and b_ == a_ - 1 and "#0  round_of_work" in out,
              "23 reverse-finish returns to the call instruction", f"{a_} -> {b_}", out)
        out = gdb(backstep, irq, [f"break replay_irq_test.c:{body}", "continue", "monitor position", "delete",
                                  "next", "next", "next", "monitor position", "reverse-next", "reverse-next",
                                  "reverse-next", "monitor position", "echo @@\\n", "info line *$pc", "x/i $pc"])
        p_a, p_b, p_c = position(out, -3), position(out, -2), position(out, -1)
        check(at_line_start(out, p_a, p_b, p_c), "24 next x3, then reverse-next x3: back to the start of a line",
              f"{p_a} -> {p_b} -> {p_c}", out)
        out = gdb(backstep, irq, [f"break replay_irq_test.c:{body}", "continue", "monitor position", "delete",
                                  "step", "step", "step", "monitor position", "reverse-step", "reverse-step",
                                  "reverse-step", "monitor position", "echo @@\\n", "info line *$pc", "x/i $pc"])
        p_a, p_b, p_c = position(out, -3), position(out, -2), position(out, -1)
        check(at_line_start(out, p_a, p_b, p_c), "25 step x3, then reverse-step x3: back to the start of a line",
              f"{p_a} -> {p_b} -> {p_c}", out)
        out = gdb(backstep, irq, ["break deep", "continue", "continue", "up", "echo @@\n", "info frame"])
        frame = out.split("@@")[-1]
        check("in deep (" in frame and "depth=depth@entry=6" in frame,
              "26 up shows the recursive caller's frame", "", out)
        out = gdb(backstep, irq, ["advance timer1_isr", "monitor position", "until", "until", "monitor position"])
        t1 = irq.symbols["timer1_isr"] & ~1
        first = next(p for p, s in enumerate(irq.states) if s[15] == t1)
        check(position(out, 0) == first and (position(out, -1) or 0) > first,
              "27 advance to a function, then until", f"{re.findall(r'position (\d+) of', out)}", out)

        # 28-30: memory, special registers, read-only history.
        poll = r["poll"]
        snap = next(p for k, p in m3.chunks(poll.data) if k == b"SNAP")
        base, length = struct.unpack_from("<2I", snap, 4 * 122)
        want = snap[524:524 + min(length, 64)]
        out = gdb(backstep, poll, ["echo @@\\n", f"x/{len(want)}xb {base:#x}"])
        got = bytes(int(b, 16) for b in re.findall(r"\t0x([0-9a-f]{2})", out.split("@@", 1)[-1]))
        check(got == want, "28 memory read equals the recording's snapshot", f"{got[:8].hex()} vs {want[:8].hex()}",
              out)
        out = gdb(backstep, irq, ["info registers msp psp primask basepri faultmask control"])
        vals = dict(re.findall(r"^(msp|psp|primask|basepri|faultmask|control)\s+0x([0-9a-f]+)", out, re.M))
        check(len(vals) == 6 and int(vals["msp"], 16) == irq.states[0][13] and vals["primask"] == "1",
              "29 special registers at the start (MSP = SP, PRIMASK set by bs_record_begin)", f"{vals}", out)
        out = gdb(backstep, irq, ["print g_target", "set var g_target = 7", "print g_target", "set $r0 = 1",
                                  "stepi", "monitor position", "detach"])
        vals = re.findall(r"\$\d+ = (\d+)", out)
        check(len(vals) == 2 and vals[0] == vals[1] and position(out) == 1 and "detached" in out,
              "30 the history is read-only; detach", f"{vals}", out)

        # The protocol directly: no-ack mode, and Ctrl-C stops a run.
        results.append(raw_protocol(backstep, r["irq"]))
        # GDB over TCP, as IDEs connect (extended-remote).
        results.append(over_tcp(backstep, demo, bug_line))

    ok = all(results)
    print(f"gdb-test: {sum(results)}/{len(results)} passed")
    print(f"gdb-test: {'PASS' if ok else 'FAIL'}")
    return 0 if ok else 1


def free_port() -> int:
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


def over_tcp(backstep: str, rec: Rec, bug_line: int) -> bool:
    """`backstep gdb --port`, and GDB with `target extended-remote`."""
    port = free_port()
    server = subprocess.Popen([backstep, "gdb", "--port", str(port), str(rec.elf), str(rec.path)],
                              stderr=subprocess.PIPE, text=True)
    try:
        assert server.stderr is not None
        while "waiting for GDB" not in server.stderr.readline():
            pass
        out = subprocess.run([GDB, "-batch", "-nx", "-ex", "set pagination off", "-ex", f"file {rec.elf}",
                              "-ex", f"target extended-remote localhost:{port}", "-ex", "continue",
                              "-ex", "watch g_rx.limit", "-ex", "reverse-continue", "-ex", "detach"],
                             capture_output=True, text=True, timeout=120).stdout
        ok = f"demo_overrun.c:{bug_line}" in out and "detached" in out
        print(f"{'PASS' if ok else 'FAIL'} GDB over TCP (extended-remote): the demo's reverse watchpoint")
        if not ok:
            print("    " + "\n    ".join(out.strip().splitlines()[-15:]))
        return ok
    finally:
        server.wait(timeout=30)


def raw_protocol(backstep: str, rec: Rec) -> bool:
    """Over TCP: qSupported, no-ack mode, registers, `c` then Ctrl-C."""
    port = free_port()
    server = subprocess.Popen([backstep, "gdb", "--port", str(port), str(rec.elf), str(rec.path)],
                              stderr=subprocess.PIPE, text=True)
    try:
        for _ in range(100):
            try:
                conn = socket.create_connection(("127.0.0.1", port), timeout=10)
                break
            except OSError:
                time.sleep(0.1)
        else:
            print("FAIL raw protocol: cannot connect")
            return False

        buffer = b""

        def packet(p: str) -> bytes:
            return f"${p}#{sum(p.encode()) % 256:02x}".encode()

        def recv() -> str:
            nonlocal buffer
            while (m := re.search(rb"\$([^#]*)#..", buffer)) is None:
                buffer += conn.recv(4096)
            buffer = buffer[m.end():]
            return m.group(1).decode()

        conn.sendall(packet("qSupported:swbreak+"))
        sup = recv()
        conn.sendall(b"+" + packet("QStartNoAckMode"))
        noack = recv()
        conn.sendall(b"+" + packet("g"))
        regs = recv()
        # Ctrl-C right after `c`: the run stops (T02), unless it reached
        # the end first.
        conn.sendall(packet("c") + b"\x03")
        stop = recv()
        conn.sendall(packet("qRcmd," + b"position".hex()))
        text, reply = "", recv()
        while reply.startswith("O") and reply != "OK":
            text += bytes.fromhex(reply[1:]).decode()
            reply = recv()
        conn.sendall(packet("k"))
        conn.close()
        ok = ("ReverseContinue+" in sup and noack == "OK" and len(regs) == 23 * 8
              and stop.startswith(("T02", "T05replaylog:end")) and "position" in text and reply == "OK")
        print(f"{'PASS' if ok else 'FAIL'} raw protocol over TCP: no-ack mode, registers, Ctrl-C stops a run"
              + ("" if ok else f": {sup!r} {noack!r} {len(regs)} {stop!r} {text!r} {reply!r}"))
        return ok
    finally:
        server.wait(timeout=30)


if __name__ == "__main__":
    sys.exit(main())
