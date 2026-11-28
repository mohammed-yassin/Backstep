#!/usr/bin/env python3
"""Make docs/demo.gif: the demo bug found backwards in GDB.

Records firmware/.../demo_overrun.elf on QEMU, runs a real GDB session
against `backstep gdb` on it, and draws that session as a terminal
animation: each command is typed, then GDB's actual output appears.
Nothing in the picture is made up; rerun this after changing the demo.

Usage: make-demo-gif.py --backstep <backstep> --elf <demo_overrun.elf> [--out docs/demo.gif]

Needs Pillow and the DejaVu Sans Mono font.

SPDX-License-Identifier: MIT
"""

from __future__ import annotations

import argparse
import re
import subprocess
import sys
import tempfile
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

FONT = "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf"
COMMANDS = [
    ("continue", "run to the end of the recording: the failed check"),
    ("print g_rx.limit", "the setting is wrong"),
    ("watch g_rx.limit", None),
    ("reverse-continue", "run backwards to the last write"),
    ("bt", None),
]
WIDTH, HEIGHT, SIZE, PAD = 1000, 600, 15, 14
BG, FG, PROMPT, NOTE, HILITE = (24, 26, 33), (220, 223, 228), (120, 200, 120), (140, 150, 170), (255, 205, 90)


def session(backstep: str, elf: Path, rec: Path) -> list[tuple[str, list[str]]]:
    """[(command, output lines)] of a real GDB session."""
    marker = "@@CMD@@"
    args = ["gdb-multiarch", "-batch", "-nx", "-ex", "set pagination off", "-ex", "set confirm off",
            "-ex", f"file {elf}", "-ex", f"target remote | {backstep} gdb --stdio {elf} {rec}"]
    for c, _ in COMMANDS:
        args += ["-ex", f"echo {marker}\\n", "-ex", c]
    out = subprocess.run(args, capture_output=True, text=True, timeout=120).stdout
    parts = out.split(marker + "\n")[1:]
    result = []
    for (c, _), part in zip(COMMANDS, parts):
        lines = [l.rstrip().expandtabs(8) for l in part.splitlines() if l.strip() and "detached" not in l]
        # Shorten long frame arguments so lines fit.
        lines = [re.sub(r"\(cpu=cpu@entry=[^)]*\)", "(...)", l) for l in lines]
        lines = [l.replace("targets/qemu-mps2-an386/", "") for l in lines]
        result.append((c, lines))
    return result


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--backstep", required=True)
    ap.add_argument("--elf", required=True)
    ap.add_argument("--out", default=str(Path(__file__).resolve().parent.parent / "docs/demo.gif"))
    args = ap.parse_args()
    elf = Path(args.elf).resolve()
    backstep = str(Path(args.backstep).resolve())
    with tempfile.TemporaryDirectory() as tmp:
        subprocess.run(["qemu-system-arm", "-machine", "mps2-an386", "-cpu", "cortex-m4", "-nographic",
                        "-monitor", "none", "-serial", "null", "-semihosting-config", "enable=on,target=native",
                        "-icount", "shift=0", "-kernel", str(elf)], cwd=tmp, check=True, timeout=60)
        steps = session(backstep, elf, Path(tmp) / "recording.bsr")

    font = ImageFont.truetype(FONT, SIZE)
    line_h = SIZE + 5
    rows = (HEIGHT - 2 * PAD) // line_h
    screen: list[tuple[str, tuple[int, int, int]]] = [
        ("$ backstep gdb demo_overrun.elf recording.bsr &", FG),
        ("$ gdb-multiarch demo_overrun.elf -ex 'target remote :3333'", FG),
        ("bs_record_begin (cfg=...) at ../recorder/src/record.c:134", FG),
    ]
    frames: list[Image.Image] = []
    durations: list[int] = []

    def draw(extra: str | None = None, cursor: bool = False) -> None:
        img = Image.new("RGB", (WIDTH, HEIGHT), BG)
        d = ImageDraw.Draw(img)
        shown = screen + ([(extra, PROMPT)] if extra is not None else [])
        for i, (text, colour) in enumerate(shown[-rows:]):
            y = PAD + i * line_h
            if text.startswith("(gdb) "):
                d.text((PAD, y), "(gdb) ", font=font, fill=PROMPT)
                d.text((PAD + font.getlength("(gdb) "), y), text[6:], font=font, fill=FG)
            else:
                d.text((PAD, y), text, font=font, fill=colour)
        if cursor:
            last = shown[-rows:][-1][0]
            x = PAD + font.getlength(last)
            y = PAD + (len(shown[-rows:]) - 1) * line_h
            d.rectangle([x + 1, y + 2, x + 9, y + SIZE + 2], fill=FG)
        frames.append(img)

    draw()
    durations.append(1200)
    for (command, lines), (_, note) in zip(steps, COMMANDS):
        if note:
            screen.append((f"# {note}", NOTE))
        for i in range(0, len(command) + 1, 2):
            draw("(gdb) " + command[:i], cursor=True)
            durations.append(70)
        screen.append(("(gdb) " + command, FG))
        for l in lines:
            colour = HILITE if "DEMO-BUG-STORE" in l or "Old value" in l or "New value" in l else FG
            screen.append((l, colour))
        draw("(gdb) ", cursor=True)
        durations.append(2600 if command == "reverse-continue" else 1500)
    screen.append(("# the store that corrupted the setting, in the interrupt handler", NOTE))
    draw("(gdb) ", cursor=True)
    durations.append(4000)

    out = Path(args.out)
    palette = frames[-1].convert("P", palette=Image.Palette.ADAPTIVE, colors=32)
    quantized = [f.quantize(palette=palette) for f in frames]
    quantized[0].save(out, save_all=True, append_images=quantized[1:], duration=durations, loop=0,
                      optimize=True)
    print(f"{out}: {len(frames)} frames, {out.stat().st_size // 1024} KiB")
    return 0


if __name__ == "__main__":
    sys.exit(main())
