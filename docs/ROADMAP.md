# Roadmap

Backstep is built in 13 milestones. Each milestone is delivered through
one or more pull requests, and is only marked **Done** when every
automated acceptance test passes in CI. Acceptance tests that need a real
board are tracked separately in
[HARDWARE_VERIFICATION.md](HARDWARE_VERIFICATION.md) and may be completed
after the merge.

Status legend: **Done** (merged, automated criteria met) ·
**In progress** · **Planned**.

| # | Milestone | Status |
|---|-----------|--------|
| M0 | Foundation | Done (hardware check pending) |
| M1 | Prototypes for the risky parts | Done (hardware check pending) |
| M2 | Cortex-M4 emulator | Done (hardware measurement pending) |
| M3 | First recording and replay (no interrupts) | Done (hardware check pending) |
| M4 | Interrupts and faults | Done (hardware check pending) |
| M5 | GDB with step-back, v0.1 release | Done (v0.1.0) |
| M6 | "Who wrote this?" queries | Planned |
| M7 | Real-world STM32 firmware (FPU, DMA, HAL) | Planned |
| M8 | FreeRTOS | Planned |
| M9 | Field flight recorder | Planned |
| M10 | mkdbg interoperability | Planned |
| M11 | MCP server for AI agents | Planned |
| M12 | v1.0 hardening and launch | Planned |

---

## M0: Foundation

**Goal:** a repository where every later milestone can be built and tested.

**Acceptance criteria**
- Repository layout: `recorder/` (C99), `host/` (Rust workspace),
  `firmware/`, `scripts/`, `docs/`; MIT license.
- Host tools build, lint cleanly and pass tests on Linux, macOS and
  Windows, and on the minimum supported Rust version.
- The recorder library passes host unit tests under AddressSanitizer and
  UndefinedBehaviorSanitizer.
- A QEMU self-test firmware builds with zero warnings and passes:
  startup code, UART output, SysTick interrupts, recorder CRC-32.
- The NUCLEO-F411RE blinky builds and, in QEMU's STM32F405 model, prints
  its banner and heartbeats and programs the exact RCC/GPIO register
  values the F411 needs.
- The CRC-32 is identical in C, Rust and Python's `zlib`.
- The hardware test script is itself tested against a simulated board.
- *Hardware:* blinky runs on a real NUCLEO-F411RE (LED, 115200-baud
  console, 1.000 s heartbeat timing).

## M1: Prototypes for the risky parts

**Goal:** prove or reject the three hardest ideas before building on them.

**Outcome:** peripheral-read capture is decided in
[ADR 0003](adr/0003-peripheral-read-capture.md), interrupt position in
[ADR 0004](adr/0004-interrupt-position.md) (14,035 interrupts checked
against QEMU's trace, zero errors), and the starting-state snapshot in
[ADR 0005](adr/0005-starting-state.md) (restored into a fresh machine,
identical continuation, every part shown necessary).

**Acceptance criteria**
- Peripheral-read capture: a memory-protection-unit (MPU) trap prototype
  and a compiler-instrumentation prototype both capture 100% of reads in
  a test program; the chosen approach is recorded in an ADR with its
  measured cost per access.
- Interrupt position (PC + registers + peripheral-read count + live
  stack hash) is recorded exactly and identifies the interruption point
  for at least 10,000 random interrupts in QEMU, with zero errors. Cases
  the key cannot decide are counted and handled by replay lookahead
  (ADR 0004).
- A starting-state snapshot (RAM + registers + System Control Space) can
  be captured and restored into a fresh machine with an identical
  continuation; documented in an ADR.
- *Hardware:* per-access overhead measured on NUCLEO-F411RE.

## M2: Cortex-M4 emulator

**Goal:** a Rust emulator for the ARMv7E-M Thumb/Thumb-2 integer
instruction set, exception entry and return.

**Status: done.** Part 1 (all 16-bit Thumb instructions, IT blocks, ELF
loader), part 2 (all 32-bit Thumb-2 integer and DSP instructions,
special registers, exclusives) and part 3 (exceptions: entry, return,
tail-chaining, priorities, faults, lockup, the SCB/NVIC registers) are
verified against QEMU; see [ADR 0006](adr/0006-emulator.md) and
[ADR 0007](adr/0007-exceptions.md). Floating point follows in M7.

- CI compares 1,000,000 instructions of random programs with QEMU on
  every change: the registers and xPSR before every instruction, and the
  data and stack RAM at the end of every program. The test counts
  instructions, not separate programs: each program is a random
  sequence of about 2,300 instructions with exceptions, and 460 of them
  are compared per run.
- Coverage of the decoder and exception model (`scripts/emu-coverage.sh`)
  is 93.3% of lines and 95.4% of regions, with every function executed.
- *Hardware (pending):* the STM32F411's exclusive monitor is measured by
  `scripts/hil/m2_nucleo_f411re.py`; the result sets the replay default
  for STM32F4 recordings.

**Acceptance criteria**
- 1,000,000 random instruction sequences produce identical registers,
  flags and memory to QEMU (differential testing), with zero mismatches.
- At least 90% test coverage of the instruction decoder.

## M3: First recording and replay (no interrupts)

**Status: done.**

- Part 1 adds the recording format and recorder API, the replay engine
  (`backstep replay`, `backstep inspect`), and the QEMU acceptance test
  (`scripts/replay-test.py`); see [ADR 0008](adr/0008-recording-format.md).
- Part 2 brings the same check to the NUCLEO-F411RE (`record_hw.c`): the
  board sends its recording over the UART and
  `scripts/hil/m3_nucleo_f411re.py` replays it. In CI the same image and
  script run on QEMU's netduinoplus2, and the replay must also equal
  QEMU's trace instruction for instruction
  (`scripts/qemu-nucleo-replay-smoke.py`).
- *Hardware (pending):* the run on the real board.

**Acceptance criteria**
- `backstep replay` reproduces 10 polling-only test programs instruction
  for instruction against QEMU's own execution trace.
- The self-check detects every deliberately corrupted recording.
- *Hardware:* on NUCLEO-F411RE, the replay's final registers and RAM
  checksum equal the crash dump.

## M4: Interrupts and faults

**Status: done.**

- Part 1: the recording format's version 2 adds the interrupt log (IRQS
  chunk), and replay delivers every interrupt at its recorded position
  ([ADR 0009](adr/0009-replaying-interrupts.md)). The recorder no longer
  leaves traces in the program's stack that replay cannot reproduce.
  `scripts/replay-irq-test.py` replays 100 of 100 QEMU runs exactly (about
  42,000 interrupts, 1,800 nested), plus
  10 crash recordings ended by a HardFault, and compares QEMU's trace
  with the replay's state for state, handlers included.
- Part 2 brings it to the NUCLEO-F411RE (`record_irq_hw.c`): 20 runs with
  SysTick, TIM3 and TIM4 interrupts and a run ended by a HardFault, sent
  over the UART and replayed by `scripts/hil/m4_nucleo_f411re.py`. In CI
  the same image and script run on QEMU's netduinoplus2
  (`scripts/qemu-nucleo-irq-smoke.py`).
- *Hardware (pending):* the run on the real board.

**Acceptance criteria**
- Nested interrupts from 3 sources at random rates replay exactly in
  100 of 100 QEMU runs.
- *Hardware:* 20 of 20 runs replay exactly on NUCLEO-F411RE.

## M5: GDB with step-back, v0.1 release

**Status: done** (v0.1.0).

- Part 1: replay lookahead for ambiguous interrupt keys, and interrupts
  that preempted another before its stub ran
  ([ADR 0009](adr/0009-replaying-interrupts.md)); copy-on-write emulator
  memory and watched accesses, the base for checkpoints.
- Part 2: checkpoints and reverse execution (`Timeline`), and the GDB
  server, `backstep gdb` ([ADR 0010](adr/0010-reverse-debugging.md),
  [guide](GDB.md)). `scripts/gdb-test.py` runs 30 scripted GDB sessions
  in CI, among them a reverse watchpoint that stops on the store that
  corrupted memory in `demo_overrun.c`.
- Part 3: VS Code configuration ([VSCODE.md](VSCODE.md), example
  `launch.json` and `tasks.json`), the demo GIF (made from a real
  session by `scripts/make-demo-gif.py`), and the release workflow that
  builds `backstep` for Linux, macOS (Arm and Intel) and Windows when a
  version tag is pushed. v0.1.0 is the first release.

**Acceptance criteria**
- GDB server supports continue, step, breakpoints, watchpoints,
  reverse-step, reverse-continue, reverse-next and reverse-finish.
- 30 scripted GDB sessions pass in CI; a reverse watchpoint stops on the
  exact instruction that corrupted memory.
- VS Code configuration documented; demo GIF; v0.1.0 binaries released.

## M6: "Who wrote this?" queries

**Acceptance criteria**
- `backstep why <variable>`, interrupt/fault timeline, JSON output.
- Example bugs (stack overflow, buffer overrun, interrupt/task race) are
  each traced to the correct source line by automated tests.

## M7: Real-world STM32 firmware

**Acceptance criteria**
- Floating-point unit: 1,000,000 differential tests against QEMU pass.
- DMA examples replay exactly; STM32 HAL and STM32CubeIDE projects work.
- Adding Backstep to a CubeIDE project takes 10 or fewer lines of setup.

## M8: FreeRTOS

**Acceptance criteria**
- A demo with 4 tasks, queues and an interrupt replays exactly
  (*hardware:* 20 of 20 runs on NUCLEO-F411RE).
- GDB `info threads` shows tasks correctly; a race-condition example is
  traced to its cause.

## M9: Field flight recorder

**Acceptance criteria**
- Continuous recording in a ring buffer with periodic snapshots and
  compression, within a RAM budget checked from the linker map.
- A recording survives a reset and replays correctly.
- Overhead within the targets set in M1.

## M10: mkdbg interoperability

**Acceptance criteria**
- Read and write mkdbg incident bundles; round-trip tests pass on mkdbg's
  own sample files.
- Coordination issue opened with the mkdbg maintainer.

## M11: MCP server for AI agents

**Acceptance criteria**
- Scripted MCP client tests query the timeline, ask who wrote a
  variable, and step through a recording.

## M12: v1.0 hardening and launch

**Acceptance criteria**
- A new user reaches their first replay in 5 minutes or less following
  only the README.
- Fuzz testing of the recording parser; signed releases; docs site.
- A second board supported (for example nRF52840), given a volunteer
  with hardware.
