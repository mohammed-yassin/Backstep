#!/usr/bin/env python3
"""Check recorded interrupt positions against QEMU's instruction trace.

Ground truth comes from QEMU run with
    -icount shift=0 -accel tcg,one-insn-per-tb=on -d cpu,int,nochain
which logs the full register state before every instruction and every
exception entry, tail-chain and return.

For every recorded interrupt event the check proves:
  1. Fidelity: the recorded context (r0-r12, SP, LR, PC, xPSR) equals the
     state of the interrupted code when it really resumed, and the recorded
     peripheral-read count equals the true count at that point.
  2. Uniqueness: replay injects an interrupt at the first instruction whose
     state, read count and stack hash equal the recorded key. Every earlier
     instruction since the previous interrupt with the same registers and
     read count is classified: provably harmless, ruled out by the stack
     hash, or ambiguous (needs replay lookahead).

Errors (the check fails): an event that does not match the true context,
a wrong peripheral-read count, or missing events. Ambiguous positions that
the key alone cannot decide are counted as "needs_lookahead" (see
docs/adr/0004-interrupt-position.md) and reported, not failed.

Usage: irq-position-check.py <firmware.elf> <qemu.log> <irq_events.bin>
                             [--expect-events N] [--corrupt K]
--corrupt K deliberately damages event K before checking; the check must
then fail (used to prove the checker can detect errors).

SPDX-License-Identifier: MIT
"""

from __future__ import annotations

import argparse
import re
import struct
import subprocess
import sys
from array import array
from dataclasses import dataclass, field

EVENT_WORDS = 20  # struct bs_irq_event
STACK_HASH_BYTES = 128  # BS_STACK_HASH_BYTES
STORE_MNEMONIC = re.compile(r"^(str|stm|push|vstr|vstm|vpush)", re.I)


@dataclass(eq=False)
class Group:
    """One interrupted context instance (shared by tail-chained exceptions)."""
    last_logged: int = -1  # last state logged before it was interrupted
    truth_index: int = -1
    truth_state: tuple | None = None


@dataclass(eq=False)
class Entry:
    exception: int
    position: int         # index among all exception entries
    t_index: int          # states logged before this exception was taken
    group: Group
    tailchained: bool
    matched: bool = False


@dataclass
class Trace:
    phantoms: int = 0
    pcs: array = field(default_factory=lambda: array("I"))
    regs: array = field(default_factory=lambda: array("I"))  # 16 per state
    keys: array = field(default_factory=lambda: array("q"))
    mmio_before: array = field(default_factory=lambda: array("I"))
    entries: list = field(default_factory=list)


def elf_symbols(elf: str, nm: str) -> dict[str, int]:
    out = subprocess.run([nm, elf], check=True, capture_output=True, text=True).stdout
    syms = {}
    for line in out.splitlines():
        parts = line.split()
        if len(parts) == 3:
            syms[parts[2]] = int(parts[0], 16)
    return syms


def disassemble(elf: str, objdump: str) -> dict[int, tuple[str, str, str]]:
    """pc -> (function, mnemonic, operands)."""
    out = subprocess.run([objdump, "-d", elf], check=True, capture_output=True,
                         text=True).stdout
    insns = {}
    func = ""
    head = re.compile(r"^[0-9a-f]+ <([^>]+)>:")
    insn = re.compile(r"^\s+([0-9a-f]+):\s+(?:[0-9a-f]{4} ?){1,2}\s+(\S+)\s*([^@;]*)")
    for line in out.splitlines():
        if m := head.match(line):
            func = m.group(1)
        elif m := insn.match(line):
            insns[int(m.group(1), 16)] = (func, m.group(2), m.group(3).strip())
    return insns


REG = {f"r{i}": i for i in range(16)}
REG.update({"sb": 9, "sl": 10, "fp": 11, "ip": 12, "sp": 13, "lr": 14, "pc": 15})


def classify_stores(insns) -> dict[int, tuple]:
    """Store instructions -> (base_reg, mode, offset, offset_reg, shift, bytes).

    mode: "off" ([Rn, #imm] / [Rn, Rm, lsl #s], incl. pre-index), "post"
    ([Rn], #imm), "inc" (STM/STMIA: Rn upwards), "dec" (PUSH/STMDB:
    below Rn). Returns None entries for forms we cannot compute (treated
    as writing live memory).
    """
    sizes = {"b": 1, "h": 2, "d": 8}
    stores = {}
    for pc, (_, mnem, ops) in insns.items():
        if not STORE_MNEMONIC.match(mnem):
            continue
        base = mnem.lower().split(".")[0]
        try:
            if base == "push":
                n = len(re.search(r"\{(.*)\}", ops).group(1).split(","))
                stores[pc] = (13, "dec", 0, None, 0, 4 * n)
            elif base.startswith("stm"):
                rn = REG[ops.split(",")[0].rstrip("!").strip()]
                regs = re.search(r"\{(.*)\}", ops).group(1)
                n = sum(1 + (REG[b.strip()] - REG[a.strip()] if "-" in r else 0)
                        for r in regs.split(",") for a, _, b in [r.partition("-")])
                mode = "dec" if base.startswith("stmdb") else "inc"
                stores[pc] = (rn, mode, 0, None, 0, 4 * n)
            elif base.startswith("str"):
                size = sizes.get(base[3:4], 4)
                m = re.search(r"\[(\w+)(?:, (#-?\d+|\w+)(?:, lsl #(\d))?)?\](!?)"
                              r"(?:, #(-?\d+))?", ops)
                rn = REG[m.group(1)]
                if m.group(5) is not None:                 # post-index
                    stores[pc] = (rn, "post", 0, None, 0, size)
                elif m.group(2) and m.group(2).startswith("#"):
                    stores[pc] = (rn, "off", int(m.group(2)[1:]), None, 0, size)
                elif m.group(2):                           # register offset
                    stores[pc] = (rn, "off", 0, REG[m.group(2)],
                                  int(m.group(3) or 0), size)
                else:
                    stores[pc] = (rn, "off", 0, None, 0, size)
            else:
                stores[pc] = None
        except (AttributeError, KeyError, ValueError):
            stores[pc] = None
    return stores


def store_range(store, regs) -> tuple[int, int] | None:
    """[start, end) bytes written by a store, given the registers before it."""
    if store is None:
        return None
    rn, mode, off, rm, shift, size = store
    base = regs[rn]
    if mode == "dec":
        return base - size, base
    if mode in ("inc", "post"):
        return base, base + size
    addr = (base + off + ((regs[rm] << shift) if rm is not None else 0)) & 0xFFFFFFFF
    return addr, addr + size

def parse_trace(path: str, mmio_count_pc: int) -> Trace:
    """mmio_count_pc: the instruction after which the read counter has
    incremented (counted as a read once it has executed)."""
    tr = Trace()
    stack: list[Group] = []
    returning: Group | None = None
    resume_pending: Group | None = None
    tailchain = False
    mmio = 0
    regs: list[int] = []
    with open(path, "r", errors="replace") as fh:
        for line in fh:
            c = line[0]
            if c == "R":
                # "R00=xxxxxxxx R01=... R02=... R03=..."
                regs.extend(int(line[i:i + 8], 16) for i in (4, 17, 30, 43))
                continue
            if line.startswith("XPSR="):
                xpsr = int(line[5:13], 16)
                state = (*regs[:15], regs[15], xpsr)  # r0-r12, sp, lr, pc, xpsr
                regs = []
                pc = state[15]
                idx = len(tr.pcs)
                tr.pcs.append(pc)
                tr.regs.extend(state[:16])
                tr.keys.append(hash(state))
                tr.mmio_before.append(mmio)
                if pc == mmio_count_pc:
                    mmio += 1
                if resume_pending is not None:
                    if resume_pending.truth_index < 0:
                        resume_pending.truth_index = idx
                        resume_pending.truth_state = state
                    resume_pending = None
                continue
            if line.startswith("...taking pending"):
                exc = int(line.rsplit(" ", 1)[1])
                if tailchain and returning is not None:
                    group = returning
                elif resume_pending is not None:
                    # An exception return completed and a pending interrupt
                    # was taken before the resumed code ran an instruction:
                    # it interrupts exactly the context just restored.
                    group = resume_pending
                    resume_pending = None
                else:
                    group = Group(last_logged=len(tr.pcs) - 1)
                tr.entries.append(Entry(exc, len(tr.entries), len(tr.pcs), group, tailchain))
                stack.append(group)
                tailchain = False
                returning = None
            elif line.startswith("Exception return:"):
                returning = stack.pop() if stack else None
            elif line.startswith("...tailchaining"):
                tailchain = True
            elif line.startswith("...successful exception return"):
                resume_pending = returning
                returning = None

    # QEMU with -icount logs an instruction's state when it enters it, but
    # may then take a pending interrupt before executing it; the instruction
    # is logged again when the code resumes. Such a "phantom" entry is the
    # last state before the interrupt and equals the resumption state. It
    # was never executed: drop it, and recount peripheral reads without it.
    # QEMU also logs an instruction twice in a row when -icount makes it
    # restart a translation block (e.g. before an I/O access). An executed
    # instruction always changes the state (it advances the PC), except a
    # branch to itself, which is harmless to drop. So the first of two
    # identical consecutive states is a phantom too.
    phantom = {i for i in range(len(tr.keys) - 1) if tr.keys[i] == tr.keys[i + 1]}
    for e in tr.entries:
        g = e.group
        if (not e.tailchained and g.last_logged >= 0 and g.truth_state is not None
                and tr.keys[g.last_logged] == hash(g.truth_state)):
            phantom.add(g.last_logged)
    reads = 0
    for i, pc in enumerate(tr.pcs):
        tr.mmio_before[i] = reads
        if i in phantom:
            tr.keys[i] = -1  # never matches a real key
            continue
        if pc == mmio_count_pc:
            reads += 1
    tr.phantoms = len(phantom)
    return tr


def load_events(path: str) -> list[dict]:
    data = open(path, "rb").read()
    n = len(data) // (EVENT_WORDS * 4)
    events = []
    for i in range(n):
        w = struct.unpack_from("<20I", data, i * EVENT_WORDS * 4)
        events.append({
            "exception": w[0],
            "state": (*w[1:14], w[14], w[15], w[16], w[17]),  # r0-r12 sp lr pc xpsr
            "mmio": w[18],
            "stack_hash": w[19],
        })
    return events


def classify_earlier_match(tr, stores, j, t, key_sp, stack_floor) -> str:
    """An earlier state j has the same registers and read count as the true
    interruption point t. Injecting at j instead swaps the handler with the
    instructions j..t-1. Decide whether that can change the outcome.

    - no stores between them: memory identical, harmless;
    - stores only to dead stack (below the interrupted SP): harmless;
    - stores to live memory only within the hashed stack window
      [SP, SP + STACK_HASH_BYTES): if any value changed, the recorded stack
      hash differs and replay rejects j; if none changed the states are
      identical. Either way replay is correct;
    - any other live store (typically globals of a superloop whose
      registers repeat exactly): the key alone cannot decide; replay must
      resolve it with lookahead (checkpoint, continue, verify against the
      next recorded data, rewind on mismatch). Reported, not an error.
    """
    writes = []
    for i in range(j, t):
        if tr.keys[i] == -1 or tr.pcs[i] not in stores:
            continue
        rng = store_range(stores[tr.pcs[i]], tr.regs[16 * i:16 * i + 16])
        writes.append(rng)
    if not writes:
        return "benign_same_memory"
    live = [w for w in writes if w is None or not (stack_floor <= w[0] and w[1] <= key_sp)]
    if not live:
        return "benign_dead_stack"
    if all(w is not None and key_sp <= w[0] and w[1] <= key_sp + STACK_HASH_BYTES
           for w in live):
        return "resolved_by_stack_hash"
    return "needs_lookahead"


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("elf")
    ap.add_argument("log")
    ap.add_argument("events")
    ap.add_argument("--expect-events", type=int, default=0)
    ap.add_argument("--corrupt", type=int, default=-1)
    ap.add_argument("--nm", default="arm-none-eabi-nm")
    ap.add_argument("--objdump", default="arm-none-eabi-objdump")
    args = ap.parse_args()

    syms = elf_symbols(args.elf, args.nm)
    insns = disassemble(args.elf, args.objdump)
    # g_mmio_reads becomes visible when mmio_read's store to it executes.
    mmio_pc = min(pc for pc, (fn, mnem, _) in insns.items()
                  if fn == "mmio_read" and mnem.startswith("str"))
    stub_pcs = {syms[s] & ~1 for s in ("SysTick_Handler", "timer0_stub")}
    stores = classify_stores(insns)
    stack_floor = syms["_ebss"]  # the stack grows down towards .bss
    tr = parse_trace(args.log, mmio_pc)
    events = load_events(args.events)
    if 0 <= args.corrupt < len(events):
        e = events[args.corrupt]
        e["mmio"] += 1  # an off-by-one read count must be caught

    errors: list[str] = []
    if args.expect_events and len(events) < args.expect_events:
        errors.append(f"expected at least {args.expect_events} events, "
                      f"file has {len(events)}")
    if len(tr.entries) != len(events):
        errors.append(f"{len(tr.entries)} exceptions in trace vs {len(events)} recorded")

    stats = {"exact": 0, "benign_same_memory": 0, "benign_dead_stack": 0,
             "resolved_by_stack_hash": 0, "needs_lookahead": 0, "tailchained": 0,
             "nested": 0, "preempted_at_stub_entry": 0}
    first_unmatched = 0
    for n, ev in enumerate(events):
        # Recording order equals entry order except when a higher-priority
        # interrupt arrives before the outer stub's first instruction; look
        # a few entries ahead for that case.
        entry = None
        for cand in tr.entries[first_unmatched:first_unmatched + 4]:
            if (not cand.matched and cand.exception == ev["exception"]
                    and cand.group.truth_state == ev["state"]):
                entry = cand
                break
        if entry is None:
            errors.append(f"event {n} (exception {ev['exception']}, pc "
                          f"{ev['state'][15]:08x}): no matching interrupt in trace")
            continue
        entry.matched = True
        while first_unmatched < len(tr.entries) and tr.entries[first_unmatched].matched:
            first_unmatched += 1

        # Global count of reads made before this exception was taken
        # (includes earlier handlers in a tail chain, exactly as replay
        # will have counted them when it reaches this point).
        true_reads = tr.mmio_before[entry.t_index]
        if true_reads != ev["mmio"]:
            errors.append(f"event {n}: recorded {ev['mmio']} peripheral reads, "
                          f"true count {true_reads}")
            continue
        stats["tailchained"] += entry.tailchained
        stats["nested"] += (ev["state"][16] & 0x1FF) != 0
        stats["preempted_at_stub_entry"] += ev["state"][15] in stub_pcs

        # Uniqueness: earlier states since the previous exception entry.
        prev_t = 0
        if entry.position > 0:
            prev_t = tr.entries[entry.position - 1].t_index
        key = hash(ev["state"])
        key_sp = ev["state"][13]
        verdict = "exact"
        for j in range(prev_t, entry.t_index):
            if tr.keys[j] == key and tr.mmio_before[j] == ev["mmio"]:
                verdict = classify_earlier_match(tr, stores, j, entry.t_index,
                                                 key_sp, stack_floor)
                break
        stats[verdict] += 1

    print(f"trace: {len(tr.pcs)} instructions ({tr.phantoms} phantom entries "
          f"removed), {len(tr.entries)} interrupts; events: {len(events)}")
    print("  " + ", ".join(f"{k}={v}" for k, v in stats.items()))
    print(f"STATS {' '.join(f'{k}={v}' for k, v in stats.items())} events={len(events)}")
    for e in errors[:10]:
        print(f"ERROR {e}")
    if errors:
        print(f"irq-position-check: FAIL ({len(errors)} errors)")
        return 1
    print("irq-position-check: PASS")
    return 0


if __name__ == "__main__":
    sys.exit(main())
