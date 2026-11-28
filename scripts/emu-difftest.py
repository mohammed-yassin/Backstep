#!/usr/bin/env python3
"""Differential test: Backstep's emulator vs QEMU, instruction by instruction.

Generates random Thumb programs, assembles them with the GNU assembler, runs
each one on QEMU (mps2-an386, logging the register state before every
instruction) and on backstep-emu-trace, and requires the two traces to be
identical: r0-r15 and xPSR (flags, Q, GE, IT state, T bit) at every step.

The generator covers every 16-bit Thumb instruction class: shifts, add/sub
(register, 3- and 8-bit immediate), data processing, high-register
ADD/MOV/CMP (including PC and SP as operands), loads and stores of every
size and addressing form, LDR literal, ADR, SP-relative accesses, SP
adjustment, PUSH/POP, LDM/STM, extends, byte reversal, forward branches
(B, B<cond>, CBZ, CBNZ) and IT blocks of 1-4 instructions.

It also covers every 32-bit Thumb-2 integer and DSP instruction (Gen32):
data processing with modified immediates and shifted registers, ADDW/SUBW,
MOVW/MOVT, saturation, bitfields, extends, parallel add/subtract, all
multiplies and divides, every load/store addressing mode, LDRD/STRD,
LDM/STM, exclusives, TBB/TBH, B.W/BL forwards and backwards, loads into PC,
MSR/MRS and running on PSP; 32-bit instructions also appear inside IT
blocks. Coverage is counted per class and per encoding width; the run
fails if a required class never executed or an unknown one did.

Usage: emu-difftest.py --emu <backstep-emu-trace> [--programs N] [--length L]
                       [--seed S] [--jobs J] [--min-instructions M] [--keep DIR]

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
from concurrent.futures import ProcessPoolExecutor
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


DST32 = DST + HI_DST                                # 32-bit forms can write r8-r12
SRC32 = SRC + HI_DST + ["lr"]
DP_OPS = ["and", "bic", "orr", "orn", "eor", "add", "adc", "sbc", "sub", "rsb"]
SHIFTS = [("lsl", 0, 31), ("lsr", 1, 32), ("asr", 1, 32), ("ror", 1, 31)]
PAR_OPS = ["add16", "asx", "sax", "sub16", "add8", "sub8"]
SYSREGS = ["apsr", "iapsr", "eapsr", "xpsr", "ipsr", "epsr", "iepsr", "msp", "psp", "primask",
           "basepri", "basepri_max", "faultmask", "control"]


class Gen32(Gen):
    """32-bit Thumb-2 instructions (single, usable in IT) and blocks."""

    def __init__(self, rnd: random.Random) -> None:
        super().__init__(rnd)
        self.label = 0
        self.have_pool = False

    def lbl(self) -> str:
        self.label += 1
        return f".Lg{self.label}"

    def fs(self, base: str, cond: str | None, flags: bool) -> str:
        """Optional S then optional condition: "adds", "addseq", "addeq"."""
        return base + ("s" if flags else "") + (cond or "")

    def modimm(self) -> int:
        r = self.r
        k = r.randrange(5)
        x = r.randrange(1, 256)
        if k == 0:
            return r.randrange(256)
        if k < 4:
            return x * [0, 0x00010001, 0x01000100, 0x01010101][k]
        rot = r.randrange(8, 32)
        v = 0x80 | r.randrange(128)
        return ((v >> rot) | (v << (32 - rot))) & 0xFFFFFFFF

    def shift(self, rrx: bool = True) -> str:
        r = self.r
        if rrx and r.random() < 0.1:
            return ", rrx"
        if r.random() < 0.2:
            return ""
        name, lo, hi = r.choice(SHIFTS)
        return f", {name} #{self.imm(lo, hi)}"

    def ror(self) -> str:
        return self.r.choice(["", ", ror #8", ", ror #16", ", ror #24"])

    def insn32(self, cond: str | None) -> str:
        """One random 32-bit instruction that is valid anywhere, IT included."""
        r = self.r
        c = cond or ""
        d, a, b, e = r.choice(DST32), r.choice(SRC32), r.choice(SRC32), r.choice(SRC32)
        flags = r.random() < 0.5
        # Kinds 30-35 give the instruction groups with many variants
        # (parallel, multiply, long multiply) more weight.
        kind = r.randrange(38)
        kind = {30: 14, 31: 14, 32: 16, 33: 16, 34: 16, 35: 17}.get(kind, kind)
        if kind == 36:
            if r.random() < 0.5:
                return f"pkhbt{c} {d}, {a}, {b}{r.choice(['', f', lsl #{self.imm(1, 31)}'])}"
            return f"pkhtb{c} {d}, {a}, {b}, asr #{self.imm(1, 32)}"
        if kind == 37:   # SP as an operand (or, in small word steps, as the result)
            op = r.randrange(7)
            if op == 0:
                return f"{self.fs(r.choice(['add', 'sub']), cond, flags)}.w {d}, sp, #0x{self.modimm():x}"
            if op == 1:
                return f"{self.fs(r.choice(['add', 'sub']), cond, flags)}.w {d}, sp, {b}" \
                       f"{r.choice(['', ', lsl #1', ', lsl #2', ', lsl #3', ', lsr #5', ', asr #2', ', ror #9'])}"
            if op == 2:
                return f"{self.c('mov', cond)}.w {d}, sp"
            if op == 3:
                return f"{r.choice(['cmp', 'cmn'])}{c}.w sp, {b}{self.shift(rrx=False)}"
            if op == 4:
                return f"{r.choice(['add', 'sub'])}{c}.w sp, sp, #{self.imm(0, 63) * 4}"
            if op == 5:
                return f"{r.choice(['addw', 'subw'])}{c} sp, sp, #{self.imm(0, 1023) * 4}"
            return f"{self.c('cmp', cond)}.w sp, #0x{self.modimm():x}"
        if kind == 0:
            return f"{self.fs(r.choice(DP_OPS), cond, flags)}.w {d}, {a}, #0x{self.modimm():x}"
        if kind == 1:
            return f"{self.fs(r.choice(['mov', 'mvn']), cond, flags)}.w {d}, #0x{self.modimm():x}"
        if kind == 2:
            return f"{r.choice(['tst', 'teq', 'cmn', 'cmp'])}{c}.w {a}, #0x{self.modimm():x}"
        if kind == 3:
            return f"{self.fs(r.choice(DP_OPS), cond, flags)}.w {d}, {a}, {b}{self.shift()}"
        if kind == 4:
            op = r.randrange(4)
            if op == 0:
                return f"{self.fs('mov', cond, flags)}.w {d}, {b}"
            if op == 1:
                return f"{self.fs('mvn', cond, flags)}.w {d}, {b}{self.shift()}"
            if op == 2:
                return f"{self.fs('rrx', cond, flags)}.w {d}, {b}"
            name, lo, hi = r.choice(SHIFTS)
            return f"{self.fs(name, cond, flags)}.w {d}, {b}, #{self.imm(max(lo, 1), hi)}"
        if kind == 5:
            return f"{r.choice(['tst', 'teq', 'cmn', 'cmp'])}{c}.w {a}, {b}{self.shift()}"
        if kind == 6:
            return f"{self.fs(r.choice(['lsl', 'lsr', 'asr', 'ror']), cond, flags)}.w {d}, {a}, {b}"
        if kind == 7:
            op = r.choice(["addw", "subw"])
            base = r.choice(SRC32 + ["sp"])
            return f"{op}{c} {d}, {base}, #{self.imm(0, 4095)}"
        if kind == 8:
            return f"adr{c}.w {d}, {'8b' if self.have_pool and r.random() < 0.5 else '8f'}"
        if kind == 9:
            return f"{r.choice(['movw', 'movt'])}{c} {d}, #0x{self.imm(0, 0xFFFF):x}"
        if kind == 10:
            op = r.randrange(4)
            if op == 0:
                sh = r.choice(["", f", lsl #{self.imm(0, 31)}", f", asr #{self.imm(1, 31)}"])
                return f"ssat{c} {d}, #{self.imm(1, 32)}, {a}{sh}"
            if op == 1:
                sh = r.choice(["", f", lsl #{self.imm(0, 31)}", f", asr #{self.imm(1, 31)}"])
                return f"usat{c} {d}, #{self.imm(0, 31)}, {a}{sh}"
            if op == 2:
                return f"ssat16{c} {d}, #{self.imm(1, 16)}, {a}"
            return f"usat16{c} {d}, #{self.imm(0, 15)}, {a}"
        if kind == 11:
            lsb = self.imm(0, 31)
            width = self.imm(1, 32 - lsb)
            op = r.choice(["sbfx", "ubfx", "bfi", "bfc"])
            if op == "bfc":
                return f"bfc{c} {d}, #{lsb}, #{width}"
            return f"{op}{c} {d}, {a}, #{lsb}, #{width}"
        if kind == 12:
            if r.random() < 0.5:
                op = r.choice(["sxth", "uxth", "sxtb", "uxtb"])
                return f"{op}{c}.w {d}, {b}{self.ror()}"
            op = r.choice(["sxtb16", "uxtb16"])
            return f"{op}{c} {d}, {b}{self.ror()}"
        if kind == 13:
            op = r.choice(["sxtah", "uxtah", "sxtab", "uxtab", "sxtab16", "uxtab16"])
            return f"{op}{c} {d}, {a}, {b}{self.ror()}"
        if kind == 14:
            if r.random() < 0.1:
                return f"sel{c} {d}, {a}, {b}"
            return f"{r.choice(['s', 'q', 'sh', 'u', 'uq', 'uh'])}{r.choice(PAR_OPS)}{c} {d}, {a}, {b}"
        if kind == 15:
            if r.random() < 0.5:
                return f"{r.choice(['qadd', 'qdadd', 'qsub', 'qdsub'])}{c} {d}, {a}, {b}"
            op = r.choice(["rev.w", "rev16.w", "rbit", "revsh.w", "clz"])
            base, _, wide = op.partition(".")
            return f"{base}{c}{'.w' if wide else ''} {d}, {b}"
        if kind == 16:
            op = r.randrange(9)
            xy = r.choice(["bb", "bt", "tb", "tt"])
            x = r.choice(["", "x"])
            if op == 0:
                return f"mul{c}.w {d}, {a}, {b}"
            if op == 1:
                return f"{r.choice(['mla', 'mls'])}{c} {d}, {a}, {b}, {e}"
            if op == 2:
                return f"smul{xy}{c} {d}, {a}, {b}"
            if op == 3:
                return f"smla{xy}{c} {d}, {a}, {b}, {e}"
            if op == 4:
                m = r.choice(["smuad", "smusd"])
                return f"{m}{x}{c} {d}, {a}, {b}"
            if op == 5:
                m = r.choice(["smlad", "smlsd"])
                return f"{m}{x}{c} {d}, {a}, {b}, {e}"
            if op == 6:
                yy = r.choice(["b", "t"])
                if r.random() < 0.5:
                    return f"smulw{yy}{c} {d}, {a}, {b}"
                return f"smlaw{yy}{c} {d}, {a}, {b}, {e}"
            if op == 7:
                rr = r.choice(["", "r"])
                if r.random() < 0.33:
                    return f"smmul{rr}{c} {d}, {a}, {b}"
                return f"{r.choice(['smmla', 'smmls'])}{rr}{c} {d}, {a}, {b}, {e}"
            if r.random() < 0.5:
                return f"usad8{c} {d}, {a}, {b}"
            return f"usada8{c} {d}, {a}, {b}, {e}"
        if kind == 17:
            lo, hi = r.sample(DST32, 2)
            op = r.randrange(6)
            if op == 0:
                return f"{r.choice(['smull', 'umull', 'smlal', 'umlal', 'umaal'])}{c} {lo}, {hi}, {a}, {b}"
            if op == 1:
                return f"smlal{r.choice(['bb', 'bt', 'tb', 'tt'])}{c} {lo}, {hi}, {a}, {b}"
            if op == 2:
                return f"{r.choice(['smlald', 'smlsld'])}{r.choice(['', 'x'])}{c} {lo}, {hi}, {a}, {b}"
            return f"{r.choice(['sdiv', 'udiv'])}{c} {d}, {a}, {b}"
        if kind in (18, 19):
            return self.load_store32(cond)
        if kind == 20:
            d1, d2 = r.sample(DST32, 2)
            sign = r.choice(["", "-"])
            if r.random() < 0.15:
                return f"ldrd{c} {d1}, {d2}, {'8b' if self.have_pool and r.random() < 0.5 else '8f'}"
            if r.random() < 0.5:
                return f"ldrd{c} {d1}, {d2}, [r7, #{sign}{self.imm(0, 255) * 4}]"
            return f"strd{c} {a}, {b}, [r7, #{sign}{self.imm(0, 255) * 4}]"
        if kind == 21:
            return f"{r.choice(['nop', 'yield', 'sev'])}{c}.w" if r.random() < 0.5 else \
                f"{r.choice(['dmb', 'dsb', 'isb', 'clrex'])}{c}"
        if kind == 22:
            return f"mrs{c} {d}, {r.choice(SYSREGS)}"
        if kind == 23:
            regs = ["apsr_nzcvq", "apsr_g", "apsr_nzcvqg", "primask", "basepri", "basepri_max", "psp"]
            if not getattr(self, "no_faultmask", False):
                regs.append("faultmask")   # (a fault with FAULTMASK set locks up)
            reg = r.choice(regs)
            return f"msr{c} {reg}, {a}"
        if kind == 24:   # PUSH.W/POP.W, one register (STR/LDR T4) or several (STMDB/LDM)
            if r.random() < 0.5:
                if r.random() < 0.5:
                    return f"push{c}.w {{{a}}}"
                return f"pop{c}.w {{{d}}}"
            if r.random() < 0.5:
                regs = sorted(r.sample(range(13), self.imm(2, 5))) + ([14] if r.random() < 0.3 else [])
                return f"push{c}.w {{{', '.join('lr' if i == 14 else f'r{i}' for i in regs)}}}"
            regs = sorted(r.sample([0, 1, 2, 3, 4, 5, 8, 9, 10, 11, 12], self.imm(2, 5)))
            return f"pop{c}.w {{{', '.join(f'r{i}' for i in regs)}}}"
        if kind == 25:   # LDM/STM without writeback through r7
            regs = sorted(r.sample([0, 1, 2, 3, 4, 5, 8, 9, 10, 11, 12], self.imm(2, 5)))
            op = r.choice(["ldmia", "stmia", "ldmdb", "stmdb"])
            return f"{op}{c}.w r7, {{{', '.join(f'r{i}' for i in regs)}}}"
        if kind == 26:   # SP-relative word access
            if r.random() < 0.5:
                return f"ldr{c}.w {d}, [sp, #{self.imm(0, 4095)}]"
            return f"str{c}.w {a}, [sp, #{self.imm(0, 4095)}]"
        return f"{self.fs(r.choice(DP_OPS), cond, flags)}.w {d}, {a}, #0x{self.modimm():x}"

    def load_store32(self, cond: str | None) -> str:
        r = self.r
        c = cond or ""
        d, a = r.choice(DST32), r.choice(SRC32)
        load = r.random() < 0.6
        size = r.choice(["", "h", "b", "sh", "sb"] if load else ["", "h", "b"])
        op = ("ldr" if load else "str") + size
        reg = d if load else a
        form = r.randrange(7)
        if form == 0:
            return f"{op}{c}.w {reg}, [r7, #{self.imm(0, 4095)}]"
        if form == 1:
            return f"{op}{c}.w {reg}, [r7, #-{self.imm(1, 255)}]"
        if form == 2:
            return f"{op}{c}.w {reg}, [r7, r6, lsl #{self.imm(0, 3)}]"
        if form == 3:
            return f"{op}t{c} {reg}, [r7, #{self.imm(0, 255)}]"
        if form == 4 and load:
            target = "8b" if self.have_pool and r.random() < 0.5 else "8f"
            if size == "":
                return f"ldr{c}.w {d}, ={self.literal()}" if r.random() < 0.5 else f"ldr{c}.w {d}, {target}"
            return f"{op}{c}.w {d}, {target}"
        if form == 5:
            hint = r.choice(["pld", "pli"])
            addr = r.choice([f"[r7, #{self.imm(0, 4095)}]", f"[r7, #-{self.imm(1, 255)}]",
                             f"[r7, r6, lsl #{self.imm(0, 3)}]"])
            return f"{hint}{c} {addr}"
        return f"{op}{c}.w {reg}, [r7, #{self.imm(0, 4095)}]"

    def literal(self) -> str:
        return f"0x{(self.r.getrandbits(32) | 0x80000001) & 0xFFFFFFFD:08x}"

    def alu(self) -> str:
        """A random instruction that neither accesses memory nor uses r5
        (the address register of the blocks below)."""
        while True:
            text = self.insn32(None) if self.r.random() < 0.5 else self.insn(None)
            if not re.search(r"\[|ldr|str|push|pop|ldm|stm|adr|ADR|msr|mrs|clrex|\br5\b", text):
                return text

    def block(self) -> list[str]:
        """A multi-instruction sequence, outside IT blocks."""
        r = self.r
        kind = r.randrange(11)
        lo = [f"r{i}" for i in (0, 1, 2, 3, 4)] + HI_DST
        if kind == 10:  # clear the sticky Q flag, then an instruction that may set it
            # Operands near the saturation limits, so that about half of
            # these overflow: halfwords of large magnitude, an accumulator
            # close to +/-2^31.
            d, a, b, e = r.sample(lo, 4)
            values = []
            for reg, near in ((a, False), (b, False), (e, True)):
                if near:
                    hi = r.choice([0x7FFF, 0x7FF0, 0x8000, 0x800F])
                else:
                    hi = r.choice([0x7FFF, 0x8000, 0x4000, 0xC000, r.randrange(0x10000)])
                lo16 = r.choice([0x7FFF, 0x8000, r.randrange(0x10000)])
                values += [f"movw {reg}, #0x{lo16:x}", f"movt {reg}, #0x{hi:x}"]
            xy = r.choice(["bb", "bt", "tb", "tt"])
            x = r.choice(["", "x"])
            q_insn = r.choice([
                f"ssat {d}, #{self.imm(1, 32)}, {a}", f"usat {d}, #{self.imm(0, 31)}, {a}",
                f"ssat16 {d}, #{self.imm(1, 16)}, {a}", f"usat16 {d}, #{self.imm(0, 15)}, {a}",
                f"{r.choice(['qadd', 'qsub', 'qdadd', 'qdsub'])} {d}, {a}, {b}",
                f"smla{xy} {d}, {a}, {b}, {e}", f"smlad{x} {d}, {a}, {b}, {e}",
                f"smuad{x} {d}, {a}, {b}", f"smlsd{x} {d}, {a}, {b}, {e}",
                f"smlaw{r.choice(['b', 't'])} {d}, {a}, {b}, {e}"])
            return values + ["mrs r5, apsr", "bic.w r5, r5, #0x08000000", "msr apsr_nzcvq, r5", q_insn]
        if kind == 0:   # pre/post-indexed single load/store with writeback
            d = r.choice(lo)
            load = r.random() < 0.6
            op = ("ldr" if load else "str") + r.choice(["", "h", "b", "sh", "sb"] if load else ["", "h", "b"])
            imm = f"#{r.choice(['', '-'])}{self.imm(0, 255)}"
            addr = f"[r5, {imm}]!" if r.random() < 0.5 else f"[r5], {imm}"
            if r.random() < 0.15:   # PUSH.W / POP.W of one register is this form with SP
                return [f"{'push' if r.random() < 0.5 else 'pop'}.w {{{d}}}"]
            return ["mov.w r5, r7", f"{op}.w {d}, {addr}"]
        if kind == 1:   # LDRD/STRD with writeback
            d1, d2 = r.sample(lo, 2)
            op = "ldrd" if r.random() < 0.5 else "strd"
            imm = f"#{r.choice(['', '-'])}{self.imm(0, 255) * 4}"
            addr = f"[r5, {imm}]!" if r.random() < 0.5 else f"[r5], {imm}"
            return ["mov.w r5, r7", f"{op} {d1}, {d2}, {addr}"]
        if kind == 2:   # LDM/STM with writeback
            regs = sorted(r.sample([0, 1, 2, 3, 4, 8, 9, 10, 11, 12], self.imm(2, 6)))
            names = [f"r{i}" for i in regs]
            op = r.choice(["ldmia", "stmia", "ldmdb", "stmdb"])
            if op.startswith("stm") and r.random() < 0.3:
                names.append("lr")
            return ["mov.w r5, r7", f"{op}.w r5!, {{{', '.join(names)}}}"]
        if kind == 3:   # exclusive pair
            st, val = r.sample(lo, 2)
            dest = r.choice(lo)
            size = r.choice(["", "b", "h"])
            off = self.imm(0, 255) * 4
            mid = [self.alu() for _ in range(self.imm(0, 2))]
            variant = r.randrange(4)
            if size == "":
                ld = [f"ldrex {dest}, [r7, #{off}]"]
                addr = f"[r7, #{off if variant != 2 else (off + 4) % 1024}]"
                pre = []
            else:
                k = self.imm(0, 1023) & (~1 if size == "h" else ~0)
                pre = [f"addw r5, r7, #{k}"]
                ld = [f"ldrex{size} {dest}, [r5]"]
                addr = "[r5]"
                if variant == 2:
                    variant = 3
            if dest == st or dest == val:
                mid = []
            tail = ["clrex"] if variant == 1 else []
            return pre + ld + mid + tail + [f"strex{size} {st}, {val}, {addr}"]
        if kind == 4:   # TBB / TBH through a table after the instruction
            idx = r.choice(["r0", "r1", "r2", "r3", "r4"])
            half = r.random() < 0.5
            tab, cases = self.lbl(), [self.lbl() for _ in range(4)]
            out = [f"and.w r5, {idx}, #3",
                   f"tbh [pc, r5, lsl #1]" if half else "tbb [pc, r5]", f"{tab}:"]
            directive = ".hword" if half else ".byte"
            out.append(f"    {directive} " + ", ".join(f"({cl}-{tab})/2" for cl in cases))
            out.append(".p2align 1")
            for cl in cases:
                out += [f"{cl}:", f"    {self.alu()}"]
            return out
        if kind == 5:   # forward B.W / B<cond>.W / BL over a few instructions
            end = self.lbl()
            op = r.choice(["b.w", f"b{r.choice(CONDS)}.w", "bl"])
            return [f"{op} {end}"] + [self.alu() for _ in range(self.imm(1, 4))] + [f"{end}:"]
        if kind == 6:   # backward branches: forward jump, then back, then out
            fwd, back, end = self.lbl(), self.lbl(), self.lbl()
            op = r.choice(["b.w", f"b{r.choice(CONDS)}.w", "bl"])
            return [f"b.w {fwd}", f"{back}:", self.alu(), f"b.w {end}", f"{fwd}:", self.alu(),
                    f"{op} {back}", f"{end}:"]
        if kind == 7:   # branches as the last instruction of an IT block
            end = self.lbl()
            cnd = r.choice(CONDS)
            first = [] if r.random() < 0.5 else [f"{self.alu()}"]
            op = r.choice(["b", "bl"])
            if first and re.search(r"\b(it|cbz|cbnz|b[a-z]*\.n)\b", first[0]):
                first = []
            pattern = "t" * len(first)
            body = [f"{self.insn32(cnd)}" for _ in first]
            return [f"it{pattern} {cnd}"] + body + [f"{op}{cnd}.w {end}" if op == "b" else f"bl{cnd} {end}",
                                                     self.alu(), f"{end}:"]
        if kind == 8:   # loads into PC: POP.W, LDR.W PC, LDM
            target = self.lbl()
            form = r.randrange(6)
            out = [f"ldr.w r4, ={target}+1"]
            if form >= 4:   # BX / BLX (register)
                return out + [f"{'bx' if form == 4 else 'blx'} r4", self.alu(), f"{target}:"]
            if form == 0:
                out += ["push.w {r3, r4}", "pop.w {r2, pc}"]
            elif form == 1:
                out += ["push.w {r4}", "ldr.w pc, [sp], #4"]
            elif form == 2:
                off = self.imm(0, 255) * 4
                out += [f"str.w r4, [r7, #{off}]", f"ldr.w pc, [r7, #{off}]"]
            else:
                off = self.imm(0, 250) * 4
                out += [f"str.w r4, [r7, #{off + 4}]", f"add.w r5, r7, #{off}", "ldm.w r5, {r3, pc}"]
            return out + [self.alu(), f"{target}:"]
        # MSR MSP, or running on PSP for a moment (CONTROL.SPSEL)
        if r.random() < 0.4:
            return ["mrs r5, msp", f"sub.w r5, r5, #{self.imm(0, 64) * 4}", "msr msp, r5"]
        body = []
        for _ in range(self.imm(1, 3)):
            choice = r.random()
            if choice < 0.3:
                body.append(f"push.w {{r0, r1}}")
                body.append("pop.w {r2, r3}")
            elif choice < 0.5:
                body.append(f"mrs {r.choice(lo)}, {r.choice(['psp', 'msp', 'control'])}")
            else:
                body.append(self.alu())
        return ["mrs r5, msp", "sub.w r5, r5, #0x800", "msr psp, r5", "mov.w r5, #2", "msr control, r5",
                "isb"] + body + ["mov.w r5, #0", "msr control, r5", "isb"]


SCS = {"ICSR": 0xE000ED04, "VTOR": 0xE000ED08, "AIRCR": 0xE000ED0C, "SCR": 0xE000ED10,
       "CCR": 0xE000ED14, "SHPR1": 0xE000ED18, "SHPR2": 0xE000ED1C, "SHPR3": 0xE000ED20,
       "SHCSR": 0xE000ED24, "CFSR": 0xE000ED28, "HFSR": 0xE000ED2C, "BFAR": 0xE000ED38,
       "ISER0": 0xE000E100, "ICER0": 0xE000E180, "ISPR0": 0xE000E200, "ICPR0": 0xE000E280,
       "IABR0": 0xE000E300, "IPR0": 0xE000E400, "IPR1": 0xE000E404, "STIR": 0xE000EF00}
UNMAPPED = 0x30000000                 # a load from here is a precise BusFault on mps2-an386
IRQS = 8                              # IRQ0-7 are used; vectors 16-47 all have handlers
# Exceptions a handler may pend, in an order that only allows pending later
# ones (so handlers cannot keep re-pending each other forever).
PEND_ORDER = [16, 17, 18, 19, 20, 21, 22, 23, 15, 14, 2]


class Gen3(Gen32):
    """Exceptions: vector table, random priorities and enables, triggers
    in thread mode (SVC, PendSV/SysTick/NMI/IRQ pending, faults, masking,
    PSP, misaligned SP) and generated handlers that read their stack frame,
    pend further exceptions and return in several ways."""

    def vectors(self) -> list[str]:
        names = {2: "h_nmi", 3: "h_hardfault", 4: "h_fault", 5: "h_fault", 6: "h_fault",
                 11: "h_svc", 12: "h_spare", 14: "h_pendsv", 15: "h_systick"}
        out = []
        for n in range(2, 48):
            if n in names:
                out.append(f".word {names[n]} + 1")
            elif n >= 16:
                out.append(f".word h_irq{n - 16 if n < 16 + IRQS else 'x'} + 1")
            else:
                out.append(".word 0")
        return out

    def store(self, reg: str, value: int, size: str = "") -> list[str]:
        return [f"ldr.w r5, =0x{SCS[reg]:08x}", f"ldr.w r4, =0x{value:08x}", f"str{size}.w r4, [r5]"]

    def setup(self) -> list[str]:
        r = self.r
        out = []
        out += self.store("SHPR1", r.getrandbits(24))
        out += self.store("SHPR2", r.getrandbits(8) << 24)
        out += self.store("SHPR3", r.getrandbits(16) << 16)
        if r.random() < 0.3:   # equal priorities: the exception number decides
            same = r.getrandbits(8) * 0x01010101
            out += self.store("IPR0", same) + self.store("IPR1", same)
            out += self.store("SHPR3", same & 0xFFFF0000)
        else:
            out += self.store("IPR0", r.getrandbits(32))
            out += self.store("IPR1", r.getrandbits(32))
        out += self.store("ISER0", r.getrandbits(IRQS) | 1)
        out += self.store("AIRCR", 0x05FA0000 | (r.randrange(8) << 8))
        out += self.store("SHCSR", r.choice([0, 7, r.getrandbits(3)]) << 16)
        ccr = 0x200 if r.random() < 0.7 else 0
        ccr |= 0x10 if r.random() < 0.5 else 0          # DIV_0_TRP
        ccr |= 0x08 if r.random() < 0.2 else 0          # UNALIGN_TRP
        out += self.store("CCR", ccr)
        # With a trap enabled, random instructions can fault, and a fault with
        # FAULTMASK set is a lockup: no random FAULTMASK writes then.
        self.no_faultmask = bool(ccr & 0x18)
        return out

    def scs_read(self, dest: str) -> list[str]:
        reg = self.r.choice(["ICSR", "VTOR", "AIRCR", "SCR", "CCR", "SHPR1", "SHPR2", "SHPR3",
                             "SHCSR", "CFSR", "HFSR", "ISER0", "ISPR0", "IABR0", "IPR0", "IPR1"])
        return [f"ldr.w r5, =0x{SCS[reg]:08x}", f"ldr.w {dest}, [r5]"]

    def pend(self, after: int | None = None) -> list[str]:
        """Make an exception pending (one later than `after` in PEND_ORDER)."""
        r = self.r
        choices = PEND_ORDER if after is None else PEND_ORDER[PEND_ORDER.index(after) + 1:]
        if not choices:
            return []
        n = r.choice(choices)
        if n == 2:
            return self.store("ICSR", 1 << 31)
        if n == 14:
            return self.store("ICSR", 1 << 28)
        if n == 15:
            return self.store("ICSR", 1 << 26)
        irq = n - 16
        if r.random() < 0.5:
            return self.store("STIR", irq)
        return self.store("ISPR0", 1 << irq)

    def exc_block(self) -> list[str]:
        """A thread-mode sequence that causes or controls exceptions."""
        r = self.r
        kind = r.randrange(15)
        # A fault or SVC with FAULTMASK set would lock up: clear it first.
        if kind == 14:  # BX/BLX to an address with bit 0 clear: INVSTATE on the next instruction
            target = self.lbl()
            # The target is word-aligned: for a halfword-aligned one QEMU
            # reports UNALIGNED (its ARM-state PC check comes first), where
            # the architecture and the emulator report INVSTATE.
            return ["cpsie f", f"ldr.w r4, ={target}", f"{r.choice(['bx', 'blx'])} r4", "nop.w",
                    ".p2align 2", f"{target}:", r.choice(["nop.w", "nop.n"])]
        if kind == 0:
            return ["cpsie f", f"svc #{self.imm(0, 255)}"]
        if kind == 1:
            return self.pend()
        if kind == 2:   # one or more pending while masked, then unmasked
            mask = r.choice([["cpsid i"], ["movs.n r4, #0x40", "msr basepri, r4"]])
            unmask = ["cpsie i"] if mask == ["cpsid i"] else ["movs.n r4, #0", "msr basepri, r4"]
            pends = [line for _ in range(self.imm(1, 3)) for line in self.pend()]
            return mask + pends + [self.alu()] + unmask
        if kind == 3:   # undefined instruction (16- or 32-bit)
            return ["cpsie f", r.choice([f"udf.w #{self.imm(0, 65535)}", f"udf.n #{self.imm(0, 255)}"])]
        if kind == 4:   # division by zero (faults if CCR.DIV_0_TRP)
            return ["cpsie f", "mov.w r5, #0", f"{r.choice(['sdiv', 'udiv'])} r4, r0, r5"]
        if kind == 5:   # unaligned access by an instruction that requires alignment
            op = r.choice(["ldrd r0, r1, [r5]", "ldm.w r5, {r0, r1}", "ldrex r0, [r5]",
                           "strd r2, r3, [r5]"])
            return ["cpsie f", f"add.w r5, r7, #{r.choice([1, 2, 3])}", op]
        if kind == 6:   # precise BusFault
            return ["cpsie f", f"mov.w r5, #0x{UNMAPPED:08x}",
                    r.choice(["ldr.w r4, [r5]", "ldrb.w r4, [r5, #3]", "ldrh.w r4, [r5, #2]"])]
        if kind == 7:   # exception with the stack misaligned by 4 (frame realigned)
            return ["cpsie f", "sub.w sp, sp, #4"] + r.choice([[f"svc #{self.imm(0, 255)}"], self.pend()]) \
                + ["add.w sp, sp, #4"]
        if kind == 8:   # exception taken inside an IT block
            cnd = r.choice(CONDS)
            return ["cpsie f", "ldr.w r5, =0xE000ED04", "mov.w r4, #0x10000000",
                    f"itt {cnd}", f"str{cnd}.w r4, [r5]", f"{self.insn32(cnd)}"]
        if kind == 9:   # SVC inside an IT block
            cnd = r.choice(CONDS)
            other = self.insn(INV[cnd]) if r.random() < 0.5 else self.insn32(INV[cnd])
            if other == "ADR":
                other = f"nop{INV[cnd]}.n"
            return ["cpsie f", f"ite {cnd}", f"svc{cnd} #{self.imm(0, 255)}", other]
        if kind == 10:  # exceptions while running on the process stack
            body = [r.choice([[f"svc #{self.imm(0, 255)}"], self.pend()]) for _ in range(self.imm(1, 2))]
            flat = [line for part in body for line in part]
            return ["cpsie f", "mrs r5, msp", "sub.w r5, r5, #0x800", "msr psp, r5", "mov.w r5, #2",
                    "msr control, r5", "isb"] + flat + ["mov.w r5, #0", "msr control, r5", "isb"]
        if kind == 11:  # enable or disable an interrupt (a pending one may now be taken)
            reg = r.choice(["ISER0", "ICER0", "ICPR0"])
            return self.store(reg, 1 << r.randrange(IRQS))
        if kind == 12:
            return self.scs_read(r.choice(["r0", "r1", "r2", "r3", "r4"]))
        # priorities changed at run time
        return self.store(r.choice(["IPR0", "IPR1", "SHPR3"]), r.getrandbits(32))

    def safe_alu(self) -> str:
        """For handlers: no SP/LR/PC, and no division (which can fault: a
        fault in HardFault or NMI is a lockup)."""
        while True:
            text = self.alu()
            if not re.search(r"\bsp\b|\blr\b|\bpc\b|div", text):
                return text

    def frame_pointer(self) -> list[str]:
        return ["tst.w lr, #4", "ite eq", "mrseq r0, msp", "mrsne r0, psp"]

    def skip_faulting(self) -> list[str]:
        """Step the stacked PC over the faulting instruction (2 or 4 bytes)
        and advance the stacked ITSTATE as the instruction would have."""
        done, zero, store = self.lbl(), self.lbl(), self.lbl()
        return [
            # The stacked T bit is set again (an INVSTATE fault stacks T = 0).
            "ldr r2, [r0, #28]", "orr.w r2, r2, #0x01000000", "str r2, [r0, #28]",
            "ldr r1, [r0, #24]", "ldrh r2, [r1]", "lsrs r2, r2, #11", "cmp r2, #0x1d", "ite cs",
            "addcs r1, r1, #4", "addcc r1, r1, #2", "str r1, [r0, #24]",
            "ldr r2, [r0, #28]", "ubfx r3, r2, #25, #2", "ubfx r1, r2, #10, #6",
            "orr.w r3, r3, r1, lsl #2", "tst.w r3, #0xF", f"beq {done}", "tst.w r3, #7", f"beq {zero}",
            "and.w r1, r3, #0x1F", "lsl.w r1, r1, #1", "and.w r1, r1, #0x1F", "bic.w r3, r3, #0x1F",
            "orr.w r3, r3, r1", f"b {store}", f"{zero}:", "movs r3, #0", f"{store}:",
            "bic.w r2, r2, #0x06000000", "bic.w r2, r2, #0x0000FC00", "and.w r1, r3, #3",
            "orr.w r2, r2, r1, lsl #25", "lsr.w r1, r3, #2", "orr.w r2, r2, r1, lsl #10",
            "str r2, [r0, #28]", f"{done}:"]

    def frame_reads(self) -> list[str]:
        r = self.r
        out = []
        for _ in range(self.imm(0, 2)):
            out.append(f"ldr r{r.choice([1, 2, 3])}, [r0, #{4 * r.randrange(8)}]")
        if r.random() < 0.3:   # change a stacked register the interrupted code will see
            slot = r.choice([0, 4, 8, 12, 16])
            out += [f"ldr r1, [r0, #{slot}]", f"adds r1, #{self.imm(1, 255)}", f"str r1, [r0, #{slot}]"]
        return out

    def body(self, n: int, fixed: bool = False) -> list[str]:
        """Random handler work: register and SCS reads, pending later
        exceptions (not from NMI/HardFault), and instructions."""
        r = self.r
        out = []
        for _ in range(self.imm(0, 3)):
            choice = r.random()
            if choice < 0.2:
                out += self.scs_read(r.choice(["r1", "r2", "r3", "r4"]))
            elif choice < 0.3:
                out.append(f"mrs r{r.choice([1, 2, 3])}, {r.choice(['ipsr', 'xpsr', 'msp', 'psp', 'basepri', 'control'])}")
            elif choice < 0.55 and not fixed and n in PEND_ORDER:
                out += self.pend(after=n)
            elif choice < 0.62 and not fixed:
                out.append("cpsid f")   # FAULTMASK is cleared by the exception return
            else:
                out.append(self.safe_alu())
        return out

    def ret(self, body: list[str]) -> list[str]:
        """Return from the handler: BX LR, POP {.., pc} or LDR pc."""
        form = self.r.randrange(3)
        if form == 0:
            return body + ["bx lr"]
        if form == 1:
            return ["push {r4, lr}"] + body + ["pop {r4, pc}"]
        return ["push.w {lr}"] + body + ["ldr.w pc, [sp], #4"]

    def handlers(self) -> list[str]:
        out = []

        def handler(name: str, lines: list[str]) -> None:
            out.extend([".thumb_func", f"{name}:"] + [f"    {line}" for line in lines])

        # Faults: clear CFSR/HFSR (W1C, values left visible in r1/r2), skip the
        # faulting instruction. HardFault skips only escalated faults (CFSR set),
        # not escalated SVCs (whose stacked PC is already the next instruction).
        for name, hard in (("h_fault", False), ("h_hardfault", True)):
            noskip = self.lbl()
            lines = self.frame_pointer() + ["ldr.w r3, =0xE000ED28", "ldr r1, [r3]", "str r1, [r3]",
                                            "ldr r2, [r3, #4]", "str r2, [r3, #4]"]
            if hard:
                lines.append(f"cbz r1, {noskip}")
            lines += self.skip_faulting() + [f"{noskip}:"] + self.frame_reads()
            lines += self.ret(self.body(3, fixed=True))
            handler(name, lines)
        handler("h_nmi", self.frame_pointer() + self.frame_reads() + self.ret(self.body(2, fixed=True)))
        handler("h_svc", self.frame_pointer() + self.frame_reads() + self.ret(self.body(11)))
        handler("h_pendsv", self.frame_pointer() + self.frame_reads() + self.ret(self.body(14)))
        handler("h_systick", self.frame_pointer() + self.frame_reads() + self.ret(self.body(15)))
        for i in range(IRQS):
            handler(f"h_irq{i}", self.frame_pointer() + self.frame_reads() + self.ret(self.body(16 + i)))
        handler("h_irqx", ["bkpt 0x01"])     # never taken: IRQ8-31 are never pended
        handler("h_spare", ["bkpt 0x02"])
        return out + ["    .ltorg"]


# RAM compared at the end of every program: the data buffer (with the reach
# of negative and register offsets around it) and the stacks (MSP, PSP).
DUMP_REGIONS = [(0x20000E00, 0x3200), (0x202F0000, 0x10100)]
PARAMS = 0x20000100     # semihosting parameter block, outside the regions


def epilogue(dump: str) -> list[str]:
    """Write DUMP_REGIONS to the file `dump` with semihosting (QEMU writes it
    on the host, backstep-emu-trace the same way), then exit."""
    out = [".global epilogue", "epilogue:", "    cpsid i", f"    ldr.w r5, =0x{PARAMS:08x}",
           "    ldr.w r4, =dump_name", "    str.w r4, [r5]", "    mov.w r4, #5", "    str.w r4, [r5, #4]",
           f"    mov.w r4, #{len(dump)}", "    str.w r4, [r5, #8]", "    mov.w r0, #1", "    mov.w r1, r5",
           "    bkpt 0xab", "    str.w r0, [r5, #12]"]
    for base, size in DUMP_REGIONS:
        out += [f"    ldr.w r4, =0x{base:08x}", "    str.w r4, [r5, #16]", f"    ldr.w r4, =0x{size:08x}",
                "    str.w r4, [r5, #20]", "    mov.w r0, #5", "    add.w r1, r5, #12", "    bkpt 0xab"]
    out += ["    mov.w r0, #2", "    add.w r1, r5, #12", "    bkpt 0xab"]
    return out


def program(rnd: random.Random, length: int, lockup: bool = False, dump: str = "mem.bin") -> str:
    g = Gen3(rnd)
    lines = [".syntax unified", ".thumb", ".cpu cortex-m4",
             ".section .vectors, \"a\"", ".word 0x20400000", ".word reset + 1"] + \
        [f"    {v}" for v in g.vectors()] + \
        [".text", ".thumb_func", ".global reset", "reset:", ".global body_start"]
    # Exception priorities, enables, PRIGROUP and CCR traps, chosen at random.
    lines += [f"    {t}" for t in g.setup()]
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
                text = g.insn32(cnd) if rnd.random() < 0.5 else g.insn(cnd)
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
        elif choice < 0.20:                     # exceptions
            block = g.exc_block()
            lines += [f"    {t}" for t in block]
            emitted += len(block)
        elif choice < 0.34:                     # 32-bit sequence (branches, exclusives, ...)
            block = g.block()
            lines += [f"    {t}" if not t.endswith(":") and not t.startswith(".") else t for t in block]
            emitted += sum(1 for t in block if not t.endswith(":") and not t.startswith("."))
        else:
            text = g.insn32(None) if rnd.random() < 0.5 else g.insn(None)
            if text == "ADR":
                text = f"adr.n {rnd.choice(DST)}, 8f"
            lines.append(f"    {text}")
            emitted += 1
        since_pool += emitted - before
        if since_pool >= 80:
            # Literal pool (and ADR target) within reach, branched over.
            since_pool = 0
            label += 1
            lines += [f"    b.w 9{label}f", "    .p2align 2", "8:", f"    .word 0x{rnd.getrandbits(32):08x}",
                      f"    .word 0x{rnd.getrandbits(32):08x}", "    .ltorg", f"9{label}:"]
            g.have_pool = True
    label += 1
    lines += [f"    b.w 9{label}f", "    .p2align 2", "8:", "    .word 0x8badf00d", "    .word 0x0ddba11", "    .ltorg",
              f"9{label}:",
              # Either a semihosting exit, or a fault with FAULTMASK set,
              # which cannot be escalated: lockup (B1.5.15).
              *(["    cpsid f", "    udf.w #0"] if lockup else []), *epilogue(dump),
              "    ldr r1, =0x20026", "    movs r0, #0x18", "    bkpt 0xab", "    .ltorg",
              "dump_name:", f"    .asciz \"{dump}\"", "    .p2align 2"]   # NUL-terminated (semihosting spec)
    lines += g.handlers()
    lines += [".section .buf, \"aw\""]
    lines += [f"    .word 0x{w:08x}" for w in init_words]
    lines += [f"    .word 0x{rnd.getrandbits(32):08x}" for _ in range(1024 - len(init_words))]
    return "\n".join(lines) + "\n"


STATE = re.compile(r"^R00=")


def parse(text: str) -> list[tuple[int, ...]]:
    """States from a QEMU-format trace, consecutive duplicates removed.

    QEMU logs a state when it enters a translation block; if an exception
    became pending (an NVIC write), the block is left before its
    instruction runs, and `-d exec` logs "Stopped execution of TB chain".
    Such a state was never executed and is dropped."""
    out: list[tuple[int, ...]] = []
    regs: list[int] = []
    appended = False
    for line in text.splitlines():
        if line.startswith("R"):
            regs.extend(int(line[i:i + 8], 16) for i in (4, 17, 30, 43))
        elif line.startswith("XPSR="):
            state = (*regs, int(line[5:13], 16))
            regs = []
            appended = not out or out[-1] != state
            if appended:
                out.append(state)
        elif line.startswith("Stopped execution of TB chain") and appended:
            out.pop()
            appended = False
    return out


REQUIRED16 = {
    "lsls", "lsrs", "asrs", "adds", "subs", "movs", "cmp", "ands", "eors", "adcs", "sbcs",
    "rors", "tst", "negs", "cmn", "orrs", "muls", "bics", "mvns", "add", "mov", "ldr", "str",
    "ldrh", "strh", "ldrb", "strb", "ldrsb", "ldrsh", "push", "pop", "stmia", "ldmia", "adr",
    "sxtb", "sxth", "uxtb", "uxth", "rev", "rev16", "revsh", "it", "b", "bcond", "cbz",
    "cbnz", "nop", "sub",
    # Inside an IT block the 16-bit data-processing forms do not set flags.
    "adc", "and", "asr", "bic", "eor", "lsl", "lsr", "mul", "mvn", "neg", "orr", "ror", "sbc",
    # Interworking branches, and the exception-related instructions.
    "bx", "blx", "cpsid", "cpsie", "svc", "udf",
}
_DP = ["and", "bic", "orr", "orn", "eor", "add", "adc", "sbc", "sub", "rsb", "mov", "mvn",
       "lsl", "lsr", "asr", "ror", "lsl imm", "lsr imm", "asr imm", "ror imm", "rrx"]
_PAR = [p + o for p in ("s", "q", "sh", "u", "uq", "uh") for o in PAR_OPS]
_XY = ["bb", "bt", "tb", "tt"]
REQUIRED32 = {f"{m}.w" for m in (
    _DP + [m.replace(" ", "s ") if " " in m else m + "s" for m in _DP] + _PAR + ["smul" + x for x in _XY] + ["smla" + x for x in _XY]
    + ["smlal" + x for x in _XY] + [
        "tst", "teq", "cmn", "cmp", "pkhbt", "pkhtb", "addw", "subw", "adr", "movw", "movt", "ssat", "usat",
        "ssat16", "usat16", "sbfx", "ubfx", "bfi", "bfc", "sxth", "uxth", "sxtb", "uxtb",
        "sxtb16", "uxtb16", "sxtah", "uxtah", "sxtab", "uxtab", "sxtab16", "uxtab16", "sel",
        "qadd", "qdadd", "qsub", "qdsub", "rev", "rev16", "rbit", "revsh", "clz", "mul", "mla",
        "mls", "smuad", "smuadx", "smusd", "smusdx", "smlad", "smladx", "smlsd", "smlsdx",
        "smulwb", "smulwt", "smlawb", "smlawt", "smmul", "smmulr", "smmla", "smmlar", "smmls",
        "smmlsr", "usad8", "usada8", "smull", "umull", "smlal", "umlal", "umaal", "smlald",
        "smlaldx", "smlsld", "smlsldx", "sdiv", "udiv", "ldr", "ldrh", "ldrb", "ldrsh", "ldrsb",
        "str", "strh", "strb", "ldrt", "ldrht", "ldrbt", "ldrsht", "ldrsbt", "strt", "strht",
        "strbt", "pld", "pli", "ldrd", "strd", "ldrex", "strex", "ldrexb", "ldrexh", "strexb",
        "strexh", "tbb", "tbh", "ldmia", "stmia", "ldmdb", "stmdb", "push", "pop", "push one",
        "pop one", "pop pc", "pop one pc", "ldmia pc", "ldr pc", "b",
        "bcond", "bl", "nop", "yield", "sev", "dmb", "dsb", "isb", "clrex", "mrs", "msr", "udf"])}
REQUIRED = REQUIRED16 | REQUIRED32 | {"conditional (IT)", "conditional (IT).w"}
BASES = {k.removesuffix(".w") for k in REQUIRED}
SHIFT_OPERAND = re.compile(r", (lsl|lsr|asr|ror) #\d+$|, (rrx)$")
COND = r"(eq|ne|cs|cc|hs|lo|mi|pl|vs|vc|hi|ls|ge|lt|gt|le)"
COND_SUFFIX = re.compile(COND + "$")
COND_BRANCH = re.compile("^b" + COND + "$")


def classify(hexwords: list[str], mnemonic: str, operands: str) -> tuple[str, bool]:
    """(instruction class, conditional by IT) of one disassembled instruction."""
    wide = len(hexwords) == 2
    mn = mnemonic.split(".")[0]
    # objdump prints ADR as "add/addw/subw Rd, pc, #imm".
    if mn in ("add", "addw", "subw") and ", pc, #" in operands:
        mn = "adr"
    conditional = False
    if mn.startswith("it") and len(mn) <= 5 and set(mn[2:]) <= set("te"):
        base = "it"
    elif COND_BRANCH.match(mn):
        # B<cond>: encoding T1/T3, unless it is a 32-bit B (T4) in IT.
        if wide and int(hexwords[1], 16) & 0x1000:
            base, conditional = "b", True
        else:
            base = "bcond"
    elif mn in BASES:
        base = mn
    else:
        stripped = COND_SUFFIX.sub("", mn)
        conditional = stripped != mn and stripped in BASES
        base = stripped if conditional else mn
    if wide:
        # objdump writes the immediate shifts as MOV.W with a shifted
        # operand, and PUSH.W/POP.W as STMDB/LDMIA/STR/LDR with SP.
        shift = SHIFT_OPERAND.search(operands)
        if base in ("mov", "movs") and shift:
            flag = "s" if base == "movs" else ""
            base = f"rrx{flag}" if shift.group(2) else f"{shift.group(1)}{flag} imm"
        elif base == "stmdb" and operands.startswith("sp!"):
            base = "push"
        elif base == "ldmia" and operands.startswith("sp!"):
            base = "pop pc" if "pc}" in operands else "pop"
        elif base == "ldmia" and "pc}" in operands:
            base = "ldmia pc"
        elif base == "str" and operands.endswith("[sp, #-4]!"):
            base = "push one"
        elif base == "ldr" and operands.endswith("[sp], #4"):
            base = "pop one pc" if operands.startswith("pc") else "pop one"
        elif base == "ldr" and operands.startswith("pc,"):
            base = "ldr pc"
    return base + (".w" if wide else ""), conditional


def mnemonic_classes(elf: Path, pcs: list[int]) -> dict[str, int]:
    """Executed instruction classes, from the disassembly of executed PCs."""
    dis = subprocess.run(["arm-none-eabi-objdump", "-d", str(elf)], capture_output=True,
                         text=True).stdout
    at = {}
    for line in dis.splitlines():
        m = re.match(r"^\s+([0-9a-f]+):\s+((?:[0-9a-f]{4} ?){1,2})\s+(\S+)\s*(.*)", line)
        if m:
            at[int(m.group(1), 16)] = classify(m.group(2).split(), m.group(3), m.group(4))
    counts: dict[str, int] = {}
    for pc in pcs:
        if pc not in at:
            continue
        key, conditional = at[pc]
        counts[key] = counts.get(key, 0) + 1
        if conditional:
            it_key = "conditional (IT)" + (".w" if key.endswith(".w") else "")
            counts[it_key] = counts.get(it_key, 0) + 1
    return counts


# Exception events that must happen (counted by backstep-emu-trace, whose
# trace has just been shown identical to QEMU's).
REQUIRED_EVENTS = {
    "entry NMI", "entry HardFault", "entry BusFault", "entry UsageFault", "entry SVCall",
    "entry PendSV", "entry SysTick", "entry IRQ", "nested", "tail-chained", "escalated",
    "fault-undefinstr", "fault-invstate", "fault-unaligned", "fault-divbyzero", "fault-preciserr",
    "realigned", "process-stack", "return-handler", "return-thread-msp", "return-thread-psp",
    "lockup",
}
ENTRY_NAMES = {2: "NMI", 3: "HardFault", 4: "MemManage", 5: "BusFault", 6: "UsageFault",
               11: "SVCall", 12: "DebugMonitor", 14: "PendSV", 15: "SysTick"}


def events(stderr: str) -> dict[str, int]:
    out: dict[str, int] = {}
    for line in stderr.splitlines():
        if not line.startswith("STAT "):
            continue
        _, name, count = line.split()
        if name.startswith("entry-"):
            n = int(name[6:])
            name = "entry " + ENTRY_NAMES.get(n, "IRQ" if n >= 16 else str(n))
        out[name] = out.get(name, 0) + int(count)
    return out


def names() -> list[str]:
    return [f"r{i}" for i in range(13)] + ["sp", "lr", "pc", "xpsr"]


def check_program(n: int, seed_base: int, length: int, emu_path: str,
                  work: Path) -> tuple[str | None, int, dict[str, int], dict[str, int]]:
    """Generate, run and compare program n: (failure message or None,
    instructions compared, instruction classes, exception events)."""
    seed = seed_base * 100_000 + n
    src = work / f"p{n}.s"
    elf = work / f"p{n}.elf"
    lockup = n % 20 == 7
    events_seen: dict[str, int] = {}
    dump = f"p{n}.mem"
    (work / "emu").mkdir(exist_ok=True)
    src.write_text(program(random.Random(seed), length, lockup, dump))
    subprocess.run(["arm-none-eabi-as", "-o", str(work / f"p{n}.o"), str(src)], check=True)
    subprocess.run(["arm-none-eabi-ld", "-T", str(work / "link.ld"), "--no-warn-rwx-segments",
                    "-o", str(elf), str(work / f"p{n}.o")], check=True)
    log = work / f"p{n}.qemu.log"
    qemu = subprocess.run(["timeout", "60", "qemu-system-arm", "-machine", "mps2-an386", "-cpu",
                           "cortex-m4", "-nographic", "-monitor", "none", "-serial", "null",
                           "-semihosting-config", "enable=on,target=native",
                           "-accel", "tcg,one-insn-per-tb=on", "-d", "cpu,exec,nochain",
                           "-D", str(log), "-kernel", str(elf)], capture_output=True, cwd=work)
    fail = f"FAIL program {n} (seed {seed})"
    if qemu.returncode != 0 and not lockup:
        return f"{fail}: QEMU exited with {qemu.returncode}", 0, {}, {}
    # A bounded run: a broken emulator that loops must fail, not hang.
    emu = subprocess.run([str(Path(emu_path).resolve()), str(elf), str(20 * length + 100_000)],
                         capture_output=True, text=True, cwd=work / "emu")
    # QEMU stops at a lockup with "qemu: fatal: Lockup"; the emulator must
    # stop there too, with Stop::Lockup.
    text = log.read_text()
    locked = lockup and "Lockup" in text[-4000:] and "Lockup" in emu.stderr
    if lockup and not locked:
        return (f"{fail}: expected a lockup in QEMU and the emulator: "
                f"{emu.stderr.strip()[-200:]}"), 0, {}, {}
    if locked:
        events_seen["lockup"] = 1
    want = parse(text)
    got = parse(emu.stdout)
    symbols = {line.split()[2]: int(line.split()[0], 16) for line in
               subprocess.run(["arm-none-eabi-nm", str(elf)], capture_output=True, text=True)
               .stdout.splitlines() if len(line.split()) == 3}
    body = symbols["body_start"]
    # States are compared up to the memory dump (whose file handles may
    # differ); the dumps are compared instead.
    end = next((i for i, st in enumerate(want) if st[15] == symbols["epilogue"]), None)
    if end is not None:
        want = want[:end + 1]
        got = got[:end + 1] if len(got) > end else got
    started = False
    for i, (w, g) in enumerate(zip(want, got)):
        # Before the program sets the flags, QEMU's reset values (UNKNOWN
        # in the architecture) are not compared.
        started = started or w[15] == body
        if (w if started else w[:16]) != (g if started else g[:16]):
            diffs = [f"{nm}: qemu {a:08x} emu {b:08x}" for nm, a, b in zip(names(), w, g) if a != b]
            pc_prev = want[i - 1][15] if i else 0
            message = (f"{fail} at step {i}, after the instruction at {pc_prev:#x}: "
                       + "; ".join(diffs))
            dis = subprocess.run(["arm-none-eabi-objdump", "-d", str(elf)], capture_output=True,
                                 text=True).stdout
            for line in dis.splitlines():
                if line.strip().startswith(f"{pc_prev:x}:"):
                    message += f"\n     {line.strip()}"
            return message, 0, {}, {}
    if emu.returncode != 0 and not locked:
        return f"{fail}: emulator: {emu.stderr.strip()[-300:]}", 0, {}, {}
    if len(want) != len(got):
        return f"{fail}: {len(want)} QEMU states vs {len(got)} emulator states", 0, {}, {}
    if not locked:
        qemu_mem = (work / dump).read_bytes() if (work / dump).exists() else b""
        emu_mem = (work / "emu" / dump).read_bytes() if (work / "emu" / dump).exists() else b""
        expected = sum(size for _, size in DUMP_REGIONS)
        if len(qemu_mem) != expected or qemu_mem != emu_mem:
            offset = next((i for i, (a, b) in enumerate(zip(qemu_mem, emu_mem)) if a != b),
                          min(len(qemu_mem), len(emu_mem)))
            address = offset
            for base, size in DUMP_REGIONS:
                if address < size:
                    address += base
                    break
                address -= size
            return (f"{fail}: memory differs at {address:#010x} ({len(qemu_mem)} bytes from QEMU, "
                    f"{len(emu_mem)} from the emulator)"), 0, {}, {}
        events_seen["memory dumps identical"] = 1
        (work / dump).unlink()
        (work / "emu" / dump).unlink()
    classes = mnemonic_classes(elf, [st[15] for st in want])
    for k, v in events(emu.stderr).items():
        events_seen[k] = events_seen.get(k, 0) + v
    log.unlink()
    return None, len(want), classes, events_seen


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--emu", required=True)
    ap.add_argument("--programs", type=int, default=20)
    ap.add_argument("--length", type=int, default=1500)
    ap.add_argument("--seed", type=int, default=1)
    ap.add_argument("--jobs", type=int, default=1, help="programs checked in parallel")
    ap.add_argument("--min-instructions", type=int, default=0,
                    help="fail unless at least this many instructions were compared")
    ap.add_argument("--keep")
    args = ap.parse_args()

    total = 0
    coverage: dict[str, int] = {}
    event_counts: dict[str, int] = {}
    with tempfile.TemporaryDirectory() as tmp:
        work = Path(args.keep or tmp)
        work.mkdir(parents=True, exist_ok=True)
        (work / "link.ld").write_text(LINKER)
        jobs = [(n, args.seed, args.length, args.emu, work) for n in range(args.programs)]
        if args.jobs > 1:
            with ProcessPoolExecutor(max_workers=args.jobs) as pool:
                results = list(pool.map(check_program, *zip(*jobs)))
        else:
            results = []
            for job in jobs:
                results.append(check_program(*job))
                if results[-1][0]:
                    break
        for failure, count, classes, seen in results:
            if failure:
                print(failure)
                return 1
            total += count
            for k, v in classes.items():
                coverage[k] = coverage.get(k, 0) + v
            for k, v in seen.items():
                event_counts[k] = event_counts.get(k, 0) + v
    print("exceptions: " + ", ".join(f"{k}={event_counts[k]}" for k in sorted(event_counts)))
    required_events = REQUIRED_EVENTS if args.programs > 7 else REQUIRED_EVENTS - {"lockup"}
    missing_events = sorted(e for e in required_events if event_counts.get(e, 0) == 0)
    if missing_events:
        print(f"emu-difftest: FAIL (exception events never happened: {', '.join(missing_events)})")
        return 1
    missing = sorted(REQUIRED - set(coverage))
    print("executed: " + ", ".join(f"{k}={coverage[k]}" for k in sorted(coverage)))
    unexpected = sorted(set(coverage) - REQUIRED)
    if unexpected:
        print(f"emu-difftest: FAIL (unclassified instructions executed: {', '.join(unexpected)})")
        return 1
    if missing:
        print(f"emu-difftest: FAIL (instruction classes never executed: {', '.join(missing)})")
        least = sorted(coverage.items(), key=lambda kv: kv[1])[:10]
        print(f"(least executed: {', '.join(f'{k}={v}' for k, v in least)})")
        return 1
    if total < args.min_instructions:
        print(f"emu-difftest: FAIL (only {total} instructions compared, "
              f"{args.min_instructions} required)")
        return 1
    print(f"emu-difftest: PASS ({args.programs} programs, {total} instructions identical to QEMU, "
          f"all {len(REQUIRED)} instruction classes executed, "
          f"{sum(v for k, v in event_counts.items() if k.startswith('entry'))} exceptions taken, "
          f"all {len(required_events)} kinds of exception event)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
