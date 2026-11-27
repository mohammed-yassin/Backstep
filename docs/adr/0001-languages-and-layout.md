# ADR 0001: Languages, licence and repository layout

**Status:** Accepted (M0)

## Context

Backstep has two halves: a recorder that runs inside customer firmware on
small microcontrollers, and host tools that replay recordings and talk to
GDB, VS Code and AI agents. It is a free, open-source project meant to be
used inside commercial products.

## Decision

- **Recorder: C99**, no dependencies, no dynamic memory. C99 compiles with
  every embedded toolchain (GCC, Clang, IAR, Keil) and links into any
  firmware.
- **Host tools: Rust.** One static binary per platform, memory safety for
  a program that parses untrusted recordings, and good performance for an
  instruction-level emulator. `unsafe` code is forbidden.
- **Licence: MIT** for everything. The recorder is compiled into users'
  firmware, so a copyleft licence would block most companies. For the same
  reason the emulator is written from scratch instead of embedding Unicorn
  or QEMU (both GPL).
- **One repository** with `recorder/`, `host/`, `firmware/`, `scripts/`
  and `docs/`, so a single pull request can change the recorder, the host
  reader and their tests together.

## Consequences

- The emulator is the largest single piece of work (milestone M2), and is
  verified against QEMU by differential testing.
- Shared definitions (version, CRC-32, later the recording format) exist
  in both C and Rust; tests check that they stay identical.
