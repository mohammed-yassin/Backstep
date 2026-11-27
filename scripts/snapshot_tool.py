#!/usr/bin/env python3
"""Read a Backstep snapshot file (struct bs_snapshot_header + regions).

  snapshot_tool.py info <snapshot.bin>         print a summary, verify CRC
  snapshot_tool.py gdb  <snapshot.bin> <out> --elf <firmware.elf> [--omit PART]
                                               write GDB commands that load
                                               the snapshot into a halted
                                               target and resume it.
                                               --omit region0|region1|regs|scs
                                               leaves one part out (negative
                                               controls for the restore test)

The layout mirrors recorder/include/backstep_snapshot.h (version 1). The
Rust host tools will own this format from M3; this script is the
prototype reader used by the M1 restore test.

SPDX-License-Identifier: MIT
"""

from __future__ import annotations

import struct
import subprocess
import sys
import zlib

MAGIC = 0x504E5342
CPU_FIELDS = [f"r{i}" for i in range(13)] + [
    "sp", "lr", "pc", "xpsr", "msp", "psp", "primask", "basepri", "faultmask", "control"]


def parse(path: str) -> dict:
    data = open(path, "rb").read()
    magic, version, header_bytes, nreg = struct.unpack_from("<4I", data, 0)
    if magic != MAGIC or version != 1:
        raise SystemExit(f"{path}: not a version-1 Backstep snapshot")
    off = 16
    cpu = dict(zip(CPU_FIELDS, struct.unpack_from("<23I", data, off)))
    off += 23 * 4
    vtor, aircr, ccr, shpr1, shpr2, shpr3, shcsr = struct.unpack_from("<7I", data, off)
    off += 28
    iser = list(struct.unpack_from("<8I", data, off))
    off += 32
    ipr = list(data[off:off + 240])
    off += 240
    syst_csr, syst_rvr, mpu_ctrl = struct.unpack_from("<3I", data, off)
    off += 12
    rbar = list(struct.unpack_from("<8I", data, off))
    rasr = list(struct.unpack_from("<8I", data, off + 32))
    off += 64
    (cpacr,) = struct.unpack_from("<I", data, off)
    off += 4
    regions = [struct.unpack_from("<2I", data, off + 8 * i) for i in range(4)][:nreg]
    off += 32
    (crc,) = struct.unpack_from("<I", data, off)
    off += 4
    if off != header_bytes:
        raise SystemExit(f"header is {header_bytes} bytes, parser expected {off}")
    blobs, pos = [], header_bytes
    for addr, length in regions:
        blobs.append((addr, pos, data[pos:pos + length]))
        pos += length
    if pos != len(data):
        raise SystemExit(f"file has {len(data) - pos} unexpected trailing bytes")
    actual = 0
    for _, _, blob in blobs:
        actual = zlib.crc32(blob, actual)
    return {"cpu": cpu, "vtor": vtor, "aircr": aircr, "ccr": ccr,
            "shpr": [shpr1, shpr2, shpr3], "shcsr": shcsr, "iser": iser, "ipr": ipr,
            "syst_csr": syst_csr, "syst_rvr": syst_rvr, "mpu_ctrl": mpu_ctrl,
            "rbar": rbar, "rasr": rasr, "cpacr": cpacr, "blobs": blobs,
            "crc": crc, "crc_ok": actual == crc}


def symbol(elf: str, name: str) -> int:
    out = subprocess.run(["arm-none-eabi-nm", elf], check=True, capture_output=True,
                         text=True).stdout
    for line in out.splitlines():
        parts = line.split()
        if len(parts) == 3 and parts[2] == name:
            return int(parts[0], 16)
    raise SystemExit(f"{elf}: symbol {name} not found")


def gdb_script(snap: dict, path: str, omit: str, header_path: str, elf: str) -> str:
    """GDB commands: load RAM regions and the header, then run the
    on-target resume routine (bs_snapshot_resume), which restores the
    System Control Space (debugger writes to it are not reliable) and the
    CPU registers, and continues at the snapshot point."""
    out = []
    for n, (addr, file_off, blob) in enumerate(snap["blobs"]):
        if omit != f"region{n}":
            out.append(f"restore {path} binary {addr - file_off:#x} {file_off} "
                       f"{file_off + len(blob)}")
    hdr = symbol(elf, "bs_resume_header")
    resume = symbol(elf, "bs_snapshot_resume") & ~1
    out.append(f"restore {header_path} binary {hdr:#x}")
    scratch = (snap["cpu"]["sp"] - 256) & ~7  # dead stack below the snapshot SP
    out.append(f"set $sp = {scratch:#x}")
    out.append(f"set $r0 = {hdr:#x}")
    out.append(f"set $pc = {resume:#x}")
    return "\n".join(out) + "\n"


def header_bytes(path: str, omit: str) -> bytes:
    """The snapshot header, optionally with one part blanked (controls)."""
    data = bytearray(open(path, "rb").read())
    (size,) = struct.unpack_from("<I", data, 8)
    head = data[:size]
    if omit == "regs":
        for i in (4, 5, 6, 7, 8, 9, 10, 11):          # r4-r11 -> 0
            struct.pack_into("<I", head, 16 + 4 * i, 0)
    elif omit == "scs":
        scs_off = 16 + 23 * 4
        scs_len = 28 + 32 + 240 + 12 + 64 + 4
        head[scs_off:scs_off + scs_len] = bytes(scs_len)
        struct.pack_into("<I", head, scs_off + 8, 0x200)  # keep CCR.STKALIGN
    return bytes(head)


def main() -> int:
    if len(sys.argv) < 3 or sys.argv[1] not in ("info", "gdb"):
        print(__doc__)
        return 2
    snap = parse(sys.argv[2])
    if sys.argv[1] == "info":
        cpu = snap["cpu"]
        print(f"pc={cpu['pc']:#010x} sp={cpu['sp']:#010x} primask={cpu['primask']} "
              f"regions={[(hex(a), len(b)) for a, _, b in snap['blobs']]} "
              f"crc={'ok' if snap['crc_ok'] else 'MISMATCH'}")
        return 0 if snap["crc_ok"] else 1
    if not snap["crc_ok"]:
        print("snapshot CRC mismatch: refusing to restore", file=sys.stderr)
        return 1
    args = sys.argv[4:]
    omit = args[args.index("--omit") + 1] if "--omit" in args else ""
    elf = args[args.index("--elf") + 1]
    header_path = sys.argv[3] + ".header.bin"
    with open(header_path, "wb") as fh:
        fh.write(header_bytes(sys.argv[2], omit))
    with open(sys.argv[3], "w") as fh:
        fh.write(gdb_script(snap, sys.argv[2], omit, header_path, elf))
    return 0


if __name__ == "__main__":
    sys.exit(main())
