#!/usr/bin/env python3
"""Differential test: Backstep's emulator vs QEMU, instruction by instruction.

Generates random Thumb programs, assembles them with the GNU assembler, runs
each one on QEMU (mps2-an386, logging the register state before every
instruction) and on backstep-emu-trace, and requires the two traces to be
identical: r0-r15 and xPSR (flags, IT state, T bit) at every step.

The generator covers every 16-bit Thumb instruction class: shifts, add/sub
(register, 3- and 8-bit immediate), data processing, high-register
ADD/MOV/CMP (including PC and SP as operands), loads and stores of every
size and addressing form, LDR literal, ADR, SP-relative accesses, SP
adjustment, PUSH/POP, LDM/STM, extends, byte reversal, forward branches
(B, B<cond>, CBZ, CBNZ) and IT blocks of 1-4 instructions.

Usage: emu-difftest.py --emu <backstep-emu-trace> [--programs N] [--length L]
                       [--seed S] [--keep DIR]

SPDX-License-Identifier: MIT
"""

from __future__ import annotations

import argparse
import os
import random
import re
import subprocess
import sys
import tempfile
from pathlib import Path

CONDS = ["eq", "ne", "cs", "cc", "mi", "pl", "vs", "vc", "hi", "ls", "ge", "lt", "gt", "le"]
INV = {"eq": "ne", "ne": "eq", "cs": "cc", "cc": "cs", "mi": "pl", "pl": "mi", "vs": "vc",
       "vc": "vs", "hi": "ls", "ls": "hi", "ge": "lt", "lt": "ge", "gt": "le", "le": "gt"}
DST = ["r0", "r1", "r2", "r3", "r4", "r5"]          # data destinations (r6, r7 reserved)
SRC = DST + ["r6", "r7"]
HI_DST = ["r8", "r9", "r10", "r11", "r12"]
HI_SRC = HI_DST + ["sp", "lr"]
BUF = 0x20001000      # 4 KiB random data; r7 points at it, r6 is an index into it
LINKER = """
ENTRY(reset)
MEMORY { FLASH (rx) : ORIGIN = 0, LENGTH = 4M  RAM (rwx) : ORIGIN = 0x20000000, LENGTH = 4M }
SECTIONS {
  .text : { KEEP(*(.vectors)) *(.text*) } > FLASH
  .buf 0x20001000 : { KEEP(*(.buf)) } > RAM
}
"""


class Gen:
    """Random program generator. `cond` is the IT condition or None."""

    def __init__(self, rnd: random.Random) -> None:
        self.r = rnd

    def imm(self, lo: int, hi: int, step: int = 1) -> int:
        return self.r.randrange(lo, hi + 1, step)

    def s(self, base: str, cond: str | None) -> str:
        """Flag-setting outside IT ("adds"), conditional inside ("addeq")."""
        return f"{base}{cond}" if cond else f"{base}s"

    def c(self, base: str, cond: str | None) -> str:
        return f"{base}{cond}" if cond else base

    def insn(self, cond: str | None) -> str:
        """One random 16-bit non-branch instruction."""
        r = self.r
        d, a, b = r.choice(DST), r.choice(SRC), r.choice(SRC)
        kind = r.randrange(26)
        if kind == 0:
            return f"{self.s('lsl', cond)}.n {d}, {a}, #{self.imm(0, 31)}"
        if kind == 1:
            return f"{self.s(r.choice(['lsr', 'asr']), cond)}.n {d}, {a}, #{self.imm(1, 32)}"
        if kind == 2:
            return f"{self.s(r.choice(['add', 'sub']), cond)}.n {d}, {a}, {b}"
        if kind == 3:
            return f"{self.s(r.choice(['add', 'sub']), cond)}.n {d}, {a}, #{self.imm(0, 7)}"
        if kind == 4:
            op = r.choice(["mov", "add", "sub"])
            return f"{self.s(op, cond)}.n {d}, #{self.imm(0, 255)}"
        if kind == 5:
            return f"{self.c('cmp', cond)}.n {a}, #{self.imm(0, 255)}"
        if kind == 6:
            op = r.choice(["and", "eor", "lsl", "lsr", "asr", "adc", "sbc", "ror", "orr", "bic", "mvn"])
            return f"{self.s(op, cond)}.n {d}, {a}"
        if kind == 7:
            return f"{self.c(r.choice(['tst', 'cmp', 'cmn']), cond)}.n {a}, {b}"
        if kind == 8:
            return f"{self.s('rsb', cond)}.n {d}, {a}, #0"
        if kind == 9:
            return f"{self.s('mul', cond)}.n {d}, {a}, {d}"
        if kind == 10:   # ADD (high registers), incl. PC and SP as source
            if r.random() < 0.5:
                return f"{self.c('add', cond)}.n {r.choice(HI_DST)}, {r.choice(SRC + HI_SRC)}"
            return f"{self.c('add', cond)}.n {d}, {r.choice(HI_SRC + ['pc'])}"
        if kind == 11:   # MOV (high registers)
            if r.random() < 0.5:
                return f"{self.c('mov', cond)}.n {r.choice(HI_DST)}, {r.choice(SRC + HI_SRC)}"
            return f"{self.c('mov', cond)}.n {d}, {r.choice(HI_SRC + ['pc'])}"
        if kind == 12:   # CMP with a high register
            return f"{self.c('cmp', cond)}.n {r.choice(HI_DST)}, {r.choice(SRC)}"
        if kind == 13:   # load/store, immediate offset
            op, scale = r.choice([("ldr", 4), ("str", 4), ("ldrh", 2), ("strh", 2),
                                  ("ldrb", 1), ("strb", 1)])
            reg = d if op.startswith("ldr") else a
            return f"{self.c(op, cond)}.n {reg}, [r7, #{self.imm(0, 31) * scale}]"
        if kind == 14:   # load/store, register offset (r6 is a word-aligned index)
            op = r.choice(["ldr", "str", "ldrh", "strh", "ldrb", "strb", "ldrsb", "ldrsh"])
            reg = d if op.startswith("ldr") else a
            return f"{self.c(op, cond)}.n {reg}, [r7, r6]"
        if kind == 15:   # SP-relative
            op = r.choice(["ldr", "str"])
            reg = d if op == "ldr" else a
            return f"{self.c(op, cond)}.n {reg}, [sp, #{self.imm(0, 255) * 4}]"
        if kind == 16:
            return f"{self.c(r.choice(['add', 'sub']), cond)}.n sp, sp, #{self.imm(0, 127) * 4}"
        if kind == 17:
            return f"{self.c('add', cond)}.n {d}, sp, #{self.imm(0, 255) * 4}"
        if kind == 18:
            op = r.choice(["sxtb", "sxth", "uxtb", "uxth", "rev", "rev16", "revsh"])
            return f"{self.c(op, cond)}.n {d}, {a}"
        if kind == 19:   # PUSH (optionally with LR)
            regs = sorted(r.sample(range(8), self.imm(1, 4)))
            names = ", ".join(f"r{i}" for i in regs) + (", lr" if r.random() < 0.3 else "")
            return f"{self.c('push', cond)}.n {{{names}}}"
        if kind == 20:   # POP into data registers only
            regs = sorted(r.sample(range(6), self.imm(1, 3)))
            return f"{self.c('pop', cond)}.n {{{', '.join(f'r{i}' for i in regs)}}}"
        if kind == 21:   # LDR literal (a value no MOV can encode: forces the pool)
            return f"{self.c('ldr', cond)}.n {d}, =0x{(r.getrandbits(32) | 0x80000001) & 0xFFFFFFFD:08x}"
        if kind == 22:
            return f"{self.c('nop', cond)}.n"
        if kind == 23:   # STM/LDM through r5 (a copy of r7 made just before)
            regs = sorted(r.sample([0, 1, 2, 3, 4], self.imm(1, 4)))
            op = r.choice(["stmia", "ldmia"])
            return f"{self.s('lsl', None)}.n r5, r7, #0\n    {op}.n r5!, {{{', '.join(f'r{i}' for i in regs)}}}" \
                if cond is None else f"{self.c('nop', cond)}.n"
        if kind == 24:   # ADR to the next literal data word
            return "ADR"
        return f"{self.s('add', cond)}.n {d}, {a}, {b}"


def program(rnd: random.Random, length: int) -> str:
    g = Gen(rnd)
    lines = [".syntax unified", ".thumb", ".cpu cortex-m4",
             ".section .vectors, \"a\"", ".word 0x20400000", ".word reset + 1",
             ".text", ".thumb_func", ".global reset", "reset:", ".global body_start"]
    # Initial state, loaded from the start of the data buffer so the
    # assembler cannot turn constants into other instructions: random
    # registers, r7 = buffer, r6 = word-aligned index, SP in the middle of
    # RAM, random flags (from a CMP of two random values).
    lines += ["    ldr.n r7, 7f", "    b.n 6f", "    .p2align 2", "7:", "    .word 0x20001000", "6:"]
    init_words = [rnd.getrandbits(32) for _ in range(16)]
    init_words[6] = rnd.randrange(0, 1024, 4)
    init_words[13] = 0x20300000
    for hi, slot in (("r8", 8), ("r9", 9), ("r10", 10), ("r11", 11), ("r12", 12), ("lr", 14)):
        lines += [f"    ldr.n r0, [r7, #{slot * 4}]", f"    mov.n {hi}, r0"]
    lines += ["    ldr.n r0, [r7, #52]", "    mov.n sp, r0"]
    lines += ["    ldr.n r0, [r7, #60]", "    ldr.n r1, [r7, #56]", "    cmp.n r0, r1"]
    for i in range(7):
        lines.append(f"    ldr.n r{i}, [r7, #{i * 4}]")
    lines.append("body_start:")
    label = 0
    emitted = 0
    since_pool = 0
    while emitted < length:
        before = emitted
        choice = rnd.random()
        if choice < 0.06:                       # IT block
            n = rnd.randint(1, 4)
            first = rnd.choice(CONDS)
            pattern = "".join(rnd.choice("te") for _ in range(n - 1))
            lines.append(f"    it{pattern} {first}")
            conds = [first] + [first if p == "t" else INV[first] for p in pattern]
            for cnd in conds:
                text = g.insn(cnd)
                if text == "ADR":
                    text = f"{g.c('nop', cnd)}.n"
                lines.append(f"    {text}")
            emitted += n + 1
        elif choice < 0.12:                     # forward branch
            label += 1
            skip = rnd.randint(1, 6)
            kind = rnd.randrange(4)
            if kind == 0:
                lines.append(f"    b.n 9{label}f")
            elif kind == 1:
                lines.append(f"    b{rnd.choice(CONDS)}.n 9{label}f")
            else:
                lines.append(f"    {'cbz' if kind == 2 else 'cbnz'} {rnd.choice(DST)}, 9{label}f")
            for _ in range(skip):
                text = g.insn(None)
                lines.append(f"    {'nop.n' if text == 'ADR' else text}")
            lines.append(f"9{label}:")
            emitted += skip + 1
        else:
            text = g.insn(None)
            if text == "ADR":
                text = f"adr.n {rnd.choice(DST)}, 8f"
            lines.append(f"    {text}")
            emitted += 1
        since_pool += emitted - before
        if since_pool >= 80:
            # Literal pool (and ADR target) within reach, branched over.
            since_pool = 0
            label += 1
            lines += [f"    b.n 9{label}f", "    .p2align 2", "8:", f"    .word 0x{rnd.getrandbits(32):08x}",
                      "    .ltorg", f"9{label}:"]
    label += 1
    lines += [f"    b.n 9{label}f", "    .p2align 2", "8:", "    .word 0x8badf00d", "    .ltorg",
              f"9{label}:",
              "    ldr r1, =0x20026", "    movs r0, #0x18", "    bkpt 0xab", "    .ltorg",
              ".section .buf, \"aw\""]
    lines += [f"    .word 0x{w:08x}" for w in init_words]
    lines += [f"    .word 0x{rnd.getrandbits(32):08x}" for _ in range(1024 - len(init_words))]
    return "\n".join(lines) + "\n"


STATE = re.compile(r"^R00=")


def parse(text: str) -> list[tuple[int, ...]]:
    """States from a QEMU-format trace, consecutive duplicates removed."""
    out: list[tuple[int, ...]] = []
    regs: list[int] = []
    for line in text.splitlines():
        if line.startswith("R"):
            regs.extend(int(line[i:i + 8], 16) for i in (4, 17, 30, 43))
        elif line.startswith("XPSR="):
            state = (*regs, int(line[5:13], 16))
            regs = []
            if not out or out[-1] != state:
                out.append(state)
    return out


REQUIRED = {
    "lsls", "lsrs", "asrs", "adds", "subs", "movs", "cmp", "ands", "eors", "adcs", "sbcs",
    "rors", "tst", "negs", "cmn", "orrs", "muls", "bics", "mvns", "add", "mov", "ldr", "str",
    "ldrh", "strh", "ldrb", "strb", "ldrsb", "ldrsh", "push", "pop", "stmia", "ldmia", "adr",
    "sxtb", "sxth", "uxtb", "uxth", "rev", "rev16", "revsh", "it", "b", "bcond", "cbz",
    "cbnz", "nop", "sub", "bkpt", "conditional (IT)",
}
COND_SUFFIX = re.compile(r"(eq|ne|cs|cc|hs|lo|mi|pl|vs|vc|hi|ls|ge|lt|gt|le)$")


def mnemonic_classes(elf: Path, pcs: list[int]) -> dict[str, int]:
    """Executed instruction classes, from the disassembly of executed PCs."""
    dis = subprocess.run(["arm-none-eabi-objdump", "-d", str(elf)], capture_output=True,
                         text=True).stdout
    at = {}
    for line in dis.splitlines():
        m = re.match(r"^\s+([0-9a-f]+):\s+(?:[0-9a-f]{4} ?){1,2}\s+(\S+)\s*(.*)", line)
        if m:
            mn = m.group(2).split(".")[0]
            # objdump prints ADR as "add Rd, pc, #imm".
            at[int(m.group(1), 16)] = "adr" if mn == "add" and ", pc, #" in m.group(3) else mn
    counts: dict[str, int] = {}
    for pc in pcs:
        mn = at.get(pc)
        if mn is None:
            continue
        if mn.startswith("it") and len(mn) <= 5 and set(mn[2:]) <= set("te"):
            key = "it"
        elif mn.startswith("b") and len(mn) == 3 and COND_SUFFIX.search(mn) and mn not in ("bic", "bkpt"):
            key = "bcond"
        else:
            base = COND_SUFFIX.sub("", mn) if mn not in REQUIRED else mn
            if base != mn:
                counts["conditional (IT)"] = counts.get("conditional (IT)", 0) + 1
            key = base
        counts[key] = counts.get(key, 0) + 1
    return counts


def names() -> list[str]:
    return [f"r{i}" for i in range(13)] + ["sp", "lr", "pc", "xpsr"]


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--emu", required=True)
    ap.add_argument("--programs", type=int, default=20)
    ap.add_argument("--length", type=int, default=1500)
    ap.add_argument("--seed", type=int, default=1)
    ap.add_argument("--keep")
    args = ap.parse_args()

    total = 0
    coverage: dict[str, int] = {}
    with tempfile.TemporaryDirectory() as tmp:
        work = Path(args.keep or tmp)
        work.mkdir(parents=True, exist_ok=True)
        (work / "link.ld").write_text(LINKER)
        for n in range(args.programs):
            seed = args.seed * 100_000 + n
            src = work / f"p{n}.s"
            elf = work / f"p{n}.elf"
            src.write_text(program(random.Random(seed), args.length))
            subprocess.run(["arm-none-eabi-as", "-o", str(work / f"p{n}.o"), str(src)], check=True)
            subprocess.run(["arm-none-eabi-ld", "-T", str(work / "link.ld"), "--no-warn-rwx-segments",
                            "-o", str(elf), str(work / f"p{n}.o")], check=True)
            log = work / f"p{n}.qemu.log"
            subprocess.run(["timeout", "60", "qemu-system-arm", "-machine", "mps2-an386", "-cpu",
                            "cortex-m4", "-nographic", "-monitor", "none", "-serial", "null",
                            "-semihosting-config", "enable=on,target=native",
                            "-accel", "tcg,one-insn-per-tb=on", "-d", "cpu,nochain",
                            "-D", str(log), "-kernel", str(elf)], check=True)
            emu = subprocess.run([args.emu, str(elf)], capture_output=True, text=True)
            if emu.returncode != 0:
                print(f"FAIL program {n} (seed {seed}): emulator: {emu.stderr.strip()}")
            want = parse(log.read_text())
            got = parse(emu.stdout)
            body = int(subprocess.run(["arm-none-eabi-nm", str(elf)], capture_output=True, text=True)
                       .stdout.split(" T body_start")[0].split()[-1], 16)
            started = False
            for i, (w, g) in enumerate(zip(want, got)):
                # Before the program sets the flags, QEMU's reset values
                # (UNKNOWN in the architecture) are not compared.
                started = started or w[15] == body
                if (w if started else w[:16]) != (g if started else g[:16]):
                    diffs = [f"{nm}: qemu {a:08x} emu {b:08x}" for nm, a, b in zip(names(), w, g) if a != b]
                    pc_prev = want[i - 1][15] if i else 0
                    print(f"FAIL program {n} (seed {seed}) at step {i}, after the instruction at "
                          f"{pc_prev:#x}: " + "; ".join(diffs))
                    dis = subprocess.run(["arm-none-eabi-objdump", "-d", str(elf)], capture_output=True,
                                         text=True).stdout
                    for line in dis.splitlines():
                        if line.strip().startswith(f"{pc_prev:x}:"):
                            print(f"     {line.strip()}")
                    return 1
            if len(want) != len(got) or emu.returncode != 0:
                print(f"FAIL program {n} (seed {seed}): {len(want)} QEMU states vs {len(got)} emulator states")
                return 1
            total += len(want)
            for k, v in mnemonic_classes(elf, [st[15] for st in want]).items():
                coverage[k] = coverage.get(k, 0) + v
            log.unlink()
    missing = sorted(REQUIRED - set(coverage))
    print("executed: " + ", ".join(f"{k}={coverage[k]}" for k in sorted(coverage)))
    if missing:
        print(f"emu-difftest: FAIL (instruction classes never executed: {', '.join(missing)})")
        return 1
    print(f"emu-difftest: PASS ({args.programs} programs, {total} instructions identical to QEMU, "
          f"all {len(REQUIRED)} instruction classes executed)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
