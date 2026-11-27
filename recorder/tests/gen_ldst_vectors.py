#!/usr/bin/env python3
"""Generate decoder test vectors for recorder/src/thumb_ldst.c.

Each vector is a load/store (or non-load/store) instruction written in
assembly together with the fields the decoder must report. The GNU
assembler produces the actual machine encoding, so the C decoder is checked
against an independent implementation of the Thumb-2 encoding rules.

Usage: gen_ldst_vectors.py <output-header> [--as arm-none-eabi-as]
                                         [--objdump arm-none-eabi-objdump]

SPDX-License-Identifier: MIT
"""

from __future__ import annotations

import argparse
import itertools
import re
import subprocess
import sys
import tempfile
from dataclasses import dataclass
from pathlib import Path

OK, NOT_LDST, UNSUPPORTED, PC_OR_SP = 0, 1, 2, 3
OFFSET, PRE, POST = 0, 1, 2


@dataclass
class Vector:
    asm: str
    status: int
    length: int = 0
    is_load: int = 0
    size: int = 0
    sign: int = 0
    rt: int = 0
    rn: int = 0
    rm: int = 0
    shift: int = 0
    regoff: int = 0
    mode: int = OFFSET
    imm: int = 0


OPS = {
    # mnemonic: (is_load, size, sign)
    "ldr": (1, 4, 0), "str": (0, 4, 0),
    "ldrh": (1, 2, 0), "strh": (0, 2, 0),
    "ldrb": (1, 1, 0), "strb": (0, 1, 0),
    "ldrsh": (1, 2, 1), "ldrsb": (1, 1, 1),
}


def vectors() -> list[Vector]:
    out: list[Vector] = []
    low = [0, 1, 3, 7]
    high = [8, 9, 12, 14]

    for op, (ld, size, sign) in OPS.items():
        # 16-bit immediate offset forms (not for signed loads).
        if not sign:
            for rt, rn in itertools.product(low, low):
                for k in (0, 1, 31):
                    imm = k * size
                    out.append(Vector(f"{op}.n r{rt}, [r{rn}, #{imm}]", OK, 2, ld,
                                      size, sign, rt, rn, imm=imm))
        # 16-bit register offset forms (all eight ops).
        for rt, rn, rm in [(0, 1, 2), (7, 6, 5), (3, 3, 4)]:
            out.append(Vector(f"{op}.n r{rt}, [r{rn}, r{rm}]", OK, 2, ld, size, sign,
                              rt, rn, rm=rm, regoff=1))
        # 32-bit imm12 forms.
        for rt, rn in [(0, 1), (8, 9), (12, 14), (14, 0), (5, 12)]:
            for imm in (0, 4, 255, 256, 4095):
                out.append(Vector(f"{op}.w r{rt}, [r{rn}, #{imm}]", OK, 4, ld, size,
                                  sign, rt, rn, imm=imm))
        # 32-bit negative imm8 offset, pre- and post-indexed.
        for rt, rn in [(1, 2), (9, 8), (12, 3)]:
            for imm in (1, 4, 255):
                out.append(Vector(f"{op} r{rt}, [r{rn}, #-{imm}]", OK, 4, ld, size,
                                  sign, rt, rn, imm=-imm))
                out.append(Vector(f"{op} r{rt}, [r{rn}, #{imm}]!", OK, 4, ld, size,
                                  sign, rt, rn, mode=PRE, imm=imm))
                out.append(Vector(f"{op} r{rt}, [r{rn}, #-{imm}]!", OK, 4, ld, size,
                                  sign, rt, rn, mode=PRE, imm=-imm))
                out.append(Vector(f"{op} r{rt}, [r{rn}], #{imm}", OK, 4, ld, size,
                                  sign, rt, rn, mode=POST, imm=imm))
                out.append(Vector(f"{op} r{rt}, [r{rn}], #-{imm}", OK, 4, ld, size,
                                  sign, rt, rn, mode=POST, imm=-imm))
        # 32-bit register offset with shift.
        for rt, rn, rm in [(0, 1, 2), (8, 9, 10), (14, 12, 11)]:
            for shift in range(4):
                sh = f", lsl #{shift}" if shift else ""
                out.append(Vector(f"{op}.w r{rt}, [r{rn}, r{rm}{sh}]", OK, 4, ld, size,
                                  sign, rt, rn, rm=rm, shift=shift, regoff=1))
        # Unprivileged forms behave like a positive imm8 offset.
        out.append(Vector(f"{op}t r2, [r3, #17]", OK, 4, ld, size, sign, 2, 3, imm=17))

    # Forms that are never peripheral accesses.
    out += [
        Vector("ldr r0, [pc, #8]", PC_OR_SP),
        Vector("ldr.w r9, [pc, #-8]", PC_OR_SP),
        Vector("ldr r1, [sp, #4]", PC_OR_SP),
        Vector("str r1, [sp, #4]", PC_OR_SP),
        Vector("ldr.w r1, [sp, #8]", PC_OR_SP),
        Vector("str.w r2, [sp, #-4]!", PC_OR_SP),
        Vector("ldr.w r3, [r13], #4", PC_OR_SP),
        Vector("ldr.w pc, [r0, #4]", PC_OR_SP),
        Vector("ldr.w sp, [r0, #4]", PC_OR_SP),
        Vector("str.w sp, [r0, #4]", PC_OR_SP),
    ]
    # Load/store forms we deliberately do not emulate (reported, never guessed).
    out += [
        Vector("ldm r0!, {r1, r2}", UNSUPPORTED),
        Vector("stm r0!, {r1, r2}", UNSUPPORTED),
        Vector("ldm.w r0, {r1, r2, r8}", UNSUPPORTED),
        Vector("ldrd r0, r1, [r2, #8]", UNSUPPORTED),
        Vector("strd r0, r1, [r2, #8]", UNSUPPORTED),
        Vector("ldrex r0, [r1]", UNSUPPORTED),
        Vector("strex r0, r2, [r1]", UNSUPPORTED),
        # Writeback with Rn == Rt is UNPREDICTABLE; gas refuses it, so emit raw.
        Vector(".inst.w 0xF8511F04", UNSUPPORTED),  # ldr r1, [r1, #4]!
        Vector(".inst.w 0xF8422B04", UNSUPPORTED),  # str r2, [r2], #4
    ]
    # Not loads or stores at all.
    out += [
        Vector("adds r0, r1, r2", NOT_LDST),
        Vector("movs r0, #1", NOT_LDST),
        Vector("bx lr", NOT_LDST),
        Vector("push {r4, lr}", NOT_LDST),
        Vector("pop {r4, pc}", NOT_LDST),
        Vector("add.w r0, r1, r2", NOT_LDST),
        Vector("bl .", NOT_LDST),
        Vector("pld [r0, #8]", NOT_LDST),
        Vector("pli [r0, #8]", NOT_LDST),
        Vector("mrs r0, msp", NOT_LDST),
        Vector("nop", NOT_LDST),
    ]
    return out


def assemble(vecs: list[Vector], gas: str, objdump: str) -> list[tuple[int, int]]:
    with tempfile.TemporaryDirectory() as tmp:
        src = Path(tmp, "v.s")
        obj = Path(tmp, "v.o")
        lines = [".syntax unified", ".thumb", ".cpu cortex-m4", ".text", ".p2align 2"]
        # A label per vector keeps the objdump output easy to split.
        for i, v in enumerate(vecs):
            lines += [f"v{i}:", f"    {v.asm}"]
        src.write_text("\n".join(lines) + "\n")
        subprocess.run([gas, "-o", str(obj), str(src)], check=True)
        dump = subprocess.run([objdump, "-d", str(obj)], check=True,
                              capture_output=True, text=True).stdout

    encodings: list[tuple[int, int]] = []
    insn = re.compile(r"^\s+[0-9a-f]+:\s+([0-9a-f]{4})(?: ([0-9a-f]{4}))?\s")
    current = None
    for line in dump.splitlines():
        if m := re.match(r"^[0-9a-f]+ <v(\d+)>:", line):
            current = int(m.group(1))
            continue
        if current is not None and (m := insn.match(line)):
            hw1 = int(m.group(1), 16)
            hw2 = int(m.group(2), 16) if m.group(2) else 0
            encodings.append((hw1, hw2))
            current = None  # first instruction after each label only
    if len(encodings) != len(vecs):
        sys.exit(f"expected {len(vecs)} encodings, objdump gave {len(encodings)}")
    return encodings


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("output")
    ap.add_argument("--as", dest="gas", default="arm-none-eabi-as")
    ap.add_argument("--objdump", default="arm-none-eabi-objdump")
    args = ap.parse_args()

    vecs = vectors()
    encs = assemble(vecs, args.gas, args.objdump)
    rows = []
    for v, (hw1, hw2) in zip(vecs, encs):
        rows.append(
            f'    {{"{v.asm}", 0x{hw1:04X}u, 0x{hw2:04X}u, {v.status}, {v.length}, '
            f"{v.is_load}, {v.size}, {v.sign}, {v.rt}, {v.rn}, {v.rm}, {v.shift}, "
            f"{v.regoff}, {v.mode}, {v.imm}}},"
        )
    Path(args.output).parent.mkdir(parents=True, exist_ok=True)
    Path(args.output).write_text(
        "/* Generated by gen_ldst_vectors.py - do not edit. */\n"
        "static const struct ldst_vector k_ldst_vectors[] = {\n"
        + "\n".join(rows)
        + "\n};\n"
    )
    print(f"wrote {len(rows)} decoder vectors to {args.output}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
