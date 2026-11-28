# ADR 0010: Reverse debugging with GDB

**Status:** Accepted (M5)

## Context

M5 makes recordings debuggable: GDB must be able to stop, step, use
breakpoints and watchpoints, and run backwards (`reverse-stepi`,
`reverse-continue`, `reverse-next`, `reverse-finish`). The answer to "who
corrupted this?" should be one reverse watchpoint away. Replay (ADR 0008,
ADR 0009) only runs forwards, from the start to the end.

## Decision

**A timeline over positions** (`backstep-replay::timeline`). A position is
the number of instructions executed since the start of the recording; its
state is the machine before that instruction, with the interrupts due
there delivered (the same state a `--trace` line shows). `Timeline::new`
checks the recording as `backstep replay` does (including lookahead),
keeps the plan of where each interrupt goes, and replays it once more
taking a checkpoint every *interval* instructions: a power of two, at
least 4096, so that there are at most 2048 checkpoints. A checkpoint is a
clone of the machine; the emulator's memory is copy-on-write (1 KiB pages
in 64 KiB blocks), so it costs the pages written since the previous one.
Going to a position restores the checkpoint at or before it and runs
forwards. A step back therefore costs at most one interval.

- **Forwards** (`step`, `run`): stop at a breakpoint address, after an
  instruction that made a watched access, or at the end.
- **Backwards** (`step_back`, `run_back`): scan the intervals backwards
  from the current position. Each one is replayed from its checkpoint,
  remembering the last breakpoint position and the last instruction that
  made a watched access; the timeline stops at the latest of them, before
  that instruction. Without one it goes on to the previous interval, or
  stops at the start.
- **Watched accesses** are the program's own loads and stores, including
  exception stacking. Replay's reproduction of the capture handler's
  frames (ADR 0009) and the debugger's own reads are not.
- The recorded interrupts are delivered at their positions in the plan,
  so no key is matched again after the first check.

**A GDB remote serial protocol server** (`backstep-gdb`, `backstep gdb`).
It listens on 127.0.0.1 (port 3333 by default) or talks on stdin/stdout
(`--stdio`, for `target remote | backstep gdb --stdio ...`).

- **Target description:** the M-profile core registers, MSP and PSP, and
  PRIMASK, BASEPRI, FAULTMASK and CONTROL.
- **Supported packets:**
  - registers (`g`, `p`) and memory (`m`);
  - software and hardware breakpoints (`Z0`, `Z1`);
  - write, read and access watchpoints (`Z2`, `Z3`, `Z4`);
  - movement: `c`, `s`, `vCont`, and `bc`/`bs` for reverse execution;
  - no-ack mode, and Ctrl-C to stop a run (over TCP).
- **Read-only:** the recording is history, so writes to memory and
  registers are refused (`E01`).
- **Ends of the recording:** reported as GDB expects, `replaylog:end` and
  `replaylog:begin`; GDB prints "No more reverse-execution history".
- **`monitor` commands:**
  - `monitor position` shows the position, the end, and the number of
    interrupts delivered.
  - `monitor goto <n>|start|end` jumps. After a jump, GDB's register
    cache is stale: `maintenance flush register-cache` refreshes it.
- **Watchpoints follow GDB's Arm model.** GDB treats Arm watchpoints as
  stopping *before* the accessing instruction, and steps over it itself
  ("non-steppable" watchpoints, in both directions). So the server
  reports a forward watch stop at the state before the instruction, and a
  backward one at the state after it. GDB then steps once and shows the
  old and new values.
  - With the reverse watchpoint in the demo, GDB ends on the store that
    corrupted the variable. Reporting the timeline's own stopping points
    instead put GDB one instruction away from the store, in each
    direction; the first test run showed this.

## Evidence

`scripts/gdb-test.py` (CI) makes five recordings on QEMU:

- nested interrupts;
- keys that need lookahead;
- a crash;
- a polling program (M3);
- `demo_overrun.c`.

It then runs **30 scripted GDB sessions** (gdb-multiarch, batch mode):

| Sessions | What they show |
|---|---|
| 1-8 | At the start, after `stepi 1000`, after `stepi 1000` + `reverse-stepi 1000`, and after jumps to random positions in all four kinds of recording, every register GDB reads equals that position's state in the forward replay trace (itself equal to QEMU's own trace). |
| 9-13 | `continue` to the end; `reverse-continue` back to the start; a breakpoint's third hit; the last hits found backwards, latest first; `tbreak`. |
| 14-16 | `watch`, `rwatch`, `awatch`: each stop follows a store or load of the variable. |
| 17-19 | **The demo bug.** After the failure, `watch g_rx.limit` + `reverse-continue` stops on the `strb` of the line marked `DEMO-BUG-STORE` in `rx_isr`. The backtrace shows the interrupt and the code it interrupted, the setting is intact before the store and corrupted after it. |
| 20-21 | A crash: the recording ends in the fault handler, and the way back leads to the bad call target. |
| 22-27 | `finish`, `reverse-finish` (to the call instruction), `next`/`reverse-next`, `step`/`reverse-step`, `up` in a recursion, `advance`, `until`. |
| 28-30 | Memory reads equal the recording's snapshot; the special registers; writes are refused and `detach` works. |

A 31st check uses the protocol directly over TCP: no-ack mode, the
registers, and Ctrl-C stopping a run.

Rust tests (on every CI platform, from a recording checked into
`host/crates/backstep-replay/tests/fixtures`):

- **Every route reaches the same state.** 300 random jumps, checkpoint
  edges, and 700 steps back across interrupt entries give exactly the
  state of a plain forward replay.
- **Breakpoints and watchpoints** stop at every hit in turn, forwards and
  backwards, and backwards the timeline stops right before the store.
- **The crash recording** runs to the fault and back.
- **`backstep-gdb`'s protocol tests** check framing, checksums, no-ack
  mode, the target description, registers, memory, read-only history,
  stop replies, monitor commands and interrupts.

## Consequences

- **Startup cost:** `backstep gdb` replays the recording twice before GDB
  connects (check, then checkpoints), at about 12 M instructions/s.
- **Memory:** at most 2048 checkpoints, each the memory written in its
  interval plus a few hundred bytes.
- **No timestamps:** positions are instruction counts, not time. M6
  (queries) and the v0.1 tooling build on the same timeline.
- **Ctrl-C over `--stdio`:** a run cannot be stopped that way (GDB sends
  the interrupt on the pipe, which the server reads only between
  packets). It stops at the end of the recording anyway.
- **Source-level stepping is GDB's:** where `reverse-next` lands in
  optimised code is GDB's decision. The tests check instruction-level
  exactness, and that source-level commands end at a line start in the
  right function.
