#!/usr/bin/env python3
"""M6 acceptance test: `backstep why` and `backstep timeline` on example
bugs recorded on QEMU.

Each example firmware marks the source lines the answer must point at
with a comment (WHY-..., DEMO-BUG-STORE); the test finds those lines and
checks the JSON answers against them:

  demo_overrun.c  buffer overrun in an interrupt handler: the last write
                  to g_rx.limit is the overrunning store (DEMO-BUG-STORE),
                  in the IRQ8 (TIMER0) handler; value 100 before.
  demo_stack.c    a task's stack overflows into its state: the last write
                  to g_task.state is the push in walk() (WHY-STACK-PUSH),
                  in thread mode, flagged as through the stack pointer.
  demo_race.c     the main code and an interrupt both count in g_events:
                  a lost update whose read is WHY-RACE-READ, whose lost
                  write is the handler's WHY-RACE-ISR, overwritten by
                  WHY-RACE-STORE.

And the timeline: every recorded interrupt appears as an entry, nested
ones flagged, entries and returns balance; a crash recording shows the
HardFault with its cause (INVSTATE) at the bad call target, after the
call through the corrupted pointer (WHY-CRASH-CALL). Plus
`--at`, an untouched variable, and lookup errors.

Usage: why-test.py --backstep <backstep> --firmware <firmware/build/qemu-mps2-an386>

SPDX-License-Identifier: MIT
"""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
import tempfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
SRC = HERE.parent / "firmware/targets/qemu-mps2-an386"


def marker(file: str, tag: str) -> int:
    lines = (SRC / file).read_text().splitlines()
    found = [i + 1 for i, l in enumerate(lines) if f"/* {tag}" in l]
    assert len(found) == 1, f"{tag} in {file}: {found}"
    return found[0]


def record(elf: Path, arg: str, work: Path) -> Path:
    work.mkdir()
    semi = "enable=on,target=native" + (f",arg={arg}" if arg else "")
    subprocess.run(["timeout", "120", "qemu-system-arm", "-machine", "mps2-an386", "-cpu", "cortex-m4",
                    "-nographic", "-monitor", "none", "-serial", "null", "-semihosting-config", semi,
                    "-icount", "shift=0", "-kernel", str(elf)], cwd=work, check=False)
    return work / "recording.bsr"


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--backstep", required=True)
    ap.add_argument("--firmware", required=True)
    args = ap.parse_args()
    backstep = str(Path(args.backstep).resolve())
    fw = Path(args.firmware).resolve()
    failures = 0

    def check(ok: bool, name: str, detail: str = "") -> None:
        nonlocal failures
        print(f"{'PASS' if ok else 'FAIL'} {name}" + (f": {detail}" if detail and not ok else ""))
        failures += not ok

    def run(*a: str) -> subprocess.CompletedProcess:
        return subprocess.run([backstep, *a], capture_output=True, text=True)

    def why(elf: Path, rec: Path, var: str, *extra: str) -> dict:
        r = run("why", str(elf), str(rec), var, "--json", *extra)
        if r.returncode != 0:
            raise RuntimeError(r.stderr)
        return json.loads(r.stdout)

    def where(a: dict) -> str:
        at = a["at"]
        return f"{at.get('function')} {Path(at.get('file') or '?').name}:{at.get('line')}"

    with tempfile.TemporaryDirectory() as tmp:
        work = Path(tmp)

        # Buffer overrun in an interrupt handler.
        elf = fw / "demo_overrun.elf"
        rec = record(elf, "", work / "overrun")
        w = why(elf, rec, "g_rx.limit")
        last = w["last_write"]
        line = marker("demo_overrun.c", "DEMO-BUG-STORE")
        check(last is not None and last["at"]["line"] == line and last["at"]["function"] == "rx_isr"
              and last["context"] == {"kind": "handler", "exception": 24, "name": "IRQ8"}
              and last["before"] == 100 and last["after"] != 100 and w["value"] == last["after"],
              f"overrun: the last write to g_rx.limit is the overrunning store (line {line}, IRQ8 handler)",
              json.dumps(last))
        check(len(w["writes"]) == 1 and not w["lost_updates"] and not last["near_stack_pointer"],
              "overrun: one write, no lost update, not through the stack", json.dumps(w["writes"]))
        early = why(elf, rec, "g_rx.limit", "--at", str(last["position"]))
        check(early["last_write"] is None and early["value"] == 100,
              "overrun: --at the store's position: not written yet, still 100", json.dumps(early)[:200])
        member = why(elf, rec, "g_rx.line[15]")
        check(member["variable"]["address"] == w["variable"]["address"] - 1 and member["variable"]["size"] == 1,
              "overrun: g_rx.line[15] is the byte before g_rx.limit", json.dumps(member["variable"]))

        # Stack overflow into a task's state.
        elf = fw / "demo_stack.elf"
        rec = record(elf, "", work / "stack")
        w = why(elf, rec, "g_task.state")
        last = w["last_write"]
        line = marker("demo_stack.c", "WHY-STACK-PUSH")
        check(last is not None and last["at"]["line"] == line and last["at"]["function"] == "walk"
              and last["context"]["kind"] == "thread" and last["near_stack_pointer"]
              and last["before"] == 0x5AFE,
              f"stack: the last write to g_task.state is a push in walk() (line {line}), through the stack",
              json.dumps(last))

        # Lost update between the main code and an interrupt.
        elf = fw / "demo_race.elf"
        rec = record(elf, "", work / "race")
        w = why(elf, rec, "g_events")
        lines = {t: marker("demo_race.c", t) for t in ("WHY-RACE-READ", "WHY-RACE-ISR", "WHY-RACE-STORE")}
        lost = w["lost_updates"]
        ok = len(lost) >= 1
        if ok:
            l = lost[-1]
            ok = (l["read"]["at"]["line"] == lines["WHY-RACE-READ"] and l["read"]["context"]["kind"] == "thread"
                  and all(x["at"]["line"] == lines["WHY-RACE-ISR"] and x["context"]["kind"] == "handler"
                          for x in l["lost"])
                  and l["write"]["at"]["line"] == lines["WHY-RACE-STORE"]
                  and l["write"]["after"] == l["lost"][-1]["after"])
        check(ok, f"race: a lost update: read line {lines['WHY-RACE-READ']}, handler's write line "
              f"{lines['WHY-RACE-ISR']}, overwritten at line {lines['WHY-RACE-STORE']}",
              json.dumps(lost[-1] if lost else w)[:600])
        check(w["last_write"] == lost[-1]["write"] if lost else False,
              "race: the last write is the overwriting one", "")
        n_main = sum(1 for x in w["writes"] if x["context"]["kind"] == "thread")
        n_isr = len(w["writes"]) - n_main
        check(n_main > 0 and n_isr > 0, f"race: writes from both contexts ({n_main} main, {n_isr} handler)")

        # The timeline of an interrupt recording.
        irq_elf = fw / "replay_irq_test.elf"
        rec = record(irq_elf, "s=5:n=40", work / "irq")
        r = run("timeline", str(irq_elf), str(rec), "--json")
        ev = json.loads(r.stdout) if r.returncode == 0 else []
        entries = [e for e in ev if e["event"] == "entry"]
        returns = [e for e in ev if e["event"] == "return"]
        inspect = run("inspect", str(rec)).stdout
        recorded = int(next(l.split()[-1] for l in inspect.splitlines() if l.strip().startswith("interrupts")))
        names = {e["name"] for e in entries}
        check(ev and ev[0]["event"] == "start" and ev[-1]["event"] == "end"
              and len([e for e in entries if not e["fault"]]) == recorded and names == {"SysTick", "IRQ8", "IRQ9"},
              f"timeline: all {recorded} recorded interrupts appear, from SysTick, IRQ8, IRQ9",
              f"{len(entries)} entries, {names}")
        check(any(e["nested"] for e in entries) and len(returns) <= len(entries)
              and all(e["position"] <= f["position"] for e, f in zip(ev, ev[1:])),
              "timeline: nested entries flagged; positions in order; returns balance entries")
        human = run("timeline", str(irq_elf), str(rec)).stdout
        check("-> SysTick" in human and "<- " in human and "start" in human,
              "timeline: the text form shows entries and returns", human[:300])

        # A crash.
        rec = record(irq_elf, "c=1:n=24", work / "crash")
        ev = json.loads(run("timeline", str(irq_elf), str(rec), "--json").stdout)
        faults = [e for e in ev if e["event"] == "entry" and e["fault"]]
        call = marker("replay_irq_test.c", "WHY-CRASH-CALL")
        check(len(faults) == 1 and faults[0]["name"] == "HardFault" and faults[0]["causes"] == ["invstate"]
              and faults[0]["at"]["address"] == 0x1000 and faults[0]["previous"]["line"] == call,
              f"timeline: the crash is a HardFault (INVSTATE) at the bad call target 0x1000, after the call "
              f"through the corrupted pointer (line {call})", json.dumps(faults))

        # Untouched variables, raw addresses, and errors.
        rec = work / "irq/recording.bsr"
        w = why(irq_elf, rec, "g_target")
        check(w["last_write"] is None and w["reads"] > 0 and w["value"] == 40,
              "an untouched variable: not written in the recording, read, value 40", json.dumps(w)[:200])
        addr = w["variable"]["address"]
        raw = why(irq_elf, rec, f"0x{addr:x}:4")
        check(raw["value"] == 40 and raw["reads"] == w["reads"], "a raw address:size gives the same answer")
        for bad, why_bad in (("no_such_variable", "an unknown name"), ("g_target.x", "a member of a non-struct"),
                             ("g_buf[64]", "an index outside the array")):
            r = run("why", str(irq_elf), str(rec), bad)
            check(r.returncode == 2 and r.stderr.strip() != "", f"{why_bad} is an error (exit 2)",
                  f"exit {r.returncode}: {r.stderr.strip()}")

    print(f"why-test: {'PASS' if failures == 0 else 'FAIL'}")
    return 0 if failures == 0 else 1


if __name__ == "__main__":
    sys.exit(main())
