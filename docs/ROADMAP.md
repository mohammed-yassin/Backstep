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
| M2 | Cortex-M4 emulator | In progress (16-bit Thumb and 32-bit Thumb-2 done) |
| M3 | First recording and replay (no interrupts) | Planned |
| M4 | Interrupts and faults | Planned |
| M5 | GDB with step-back, v0.1 release | Planned |
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

**Progress:** part 1 (all 16-bit Thumb instructions, IT blocks, ELF
loader) and part 2 (all 32-bit Thumb-2 integer and DSP instructions,
special registers, exclusives) are verified against QEMU; see
[ADR 0006](adr/0006-emulator.md). Part 3 adds exceptions and the
1,000,000-instruction run. Floating point follows in M7.

**Acceptance criteria**
- 1,000,000 random instruction sequences produce identical registers,
  flags and memory to QEMU (differential testing), with zero mismatches.
- At least 90% test coverage of the instruction decoder.

## M3: First recording and replay (no interrupts)

**Acceptance criteria**
- `backstep replay` reproduces 10 polling-only test programs instruction
  for instruction against QEMU's own execution trace.
- The self-check detects every deliberately corrupted recording.
- *Hardware:* on NUCLEO-F411RE, the replay's final registers and RAM
  checksum equal the crash dump.

## M4: Interrupts and faults

**Acceptance criteria**
- Nested interrupts from 3 sources at random rates replay exactly in
  100 of 100 QEMU runs.
- *Hardware:* 20 of 20 runs replay exactly on NUCLEO-F411RE.

## M5: GDB with step-back, v0.1 release

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
