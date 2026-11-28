# Architecture

This document describes how Backstep is designed. Parts that are not built
yet are marked with the milestone that delivers them.

## The core idea: record inputs, replay execution

A Cortex-M core is deterministic. Given the same starting state and the
same inputs, it executes the same instructions in the same order. The only
sources of non-determinism in a bare-metal or RTOS firmware are:

| Source | Example | How Backstep captures it |
|--------|---------|--------------------------|
| Starting state | RAM contents, CPU registers when recording begins | Snapshot (M3, M9) |
| Peripheral reads | `USART2->DR`, `TIM2->CNT`, `ADC1->DR` | Logged value per read (M3) |
| Interrupt arrival | SysTick fires between two instructions | Logged position (M4) |
| DMA writes | ADC samples copied into RAM | Logged buffer contents (M7) |

Everything else (arithmetic, memory accesses to RAM and flash, branches)
can be recomputed. The recording therefore holds only these inputs, which
keeps it small enough for a microcontroller's RAM.

## Components

```
┌─────────────── target (MCU) ───────────────┐      ┌──────────────── host (PC) ────────────────┐
│ your firmware                              │      │ backstep CLI (Rust)                       │
│   └─ recorder (C99)                        │      │   ├─ recording reader + checks            │
│        ├─ capture peripheral reads   (M1)  │      │   ├─ Cortex-M4 emulator            (M2)   │
│        ├─ capture interrupt points   (M4)  │ ───► │   ├─ replay engine                 (M3)   │
│        ├─ capture DMA data           (M7)  │ file │   ├─ GDB server, reverse execution (M5)   │
│        ├─ state checksums (CRC-32)   (M0)  │      │   ├─ "who wrote this?" queries     (M6)   │
│        └─ ring buffer + snapshots    (M9)  │      │   └─ MCP server for AI agents      (M11)  │
└────────────────────────────────────────────┘      └───────────────────────────────────────────┘
```

### Recorder (`recorder/`, C99)

- Dependency-free C99, no dynamic memory, no RTOS requirement.
- Stores events in a caller-provided RAM buffer; the application decides
  how the recording leaves the chip (debug probe, UART, flash).
- Captures peripheral reads with an MPU trap (`mpu_trap.c`, decoder in
  `thumb_ldst.c`), or optionally with a Clang pass (`cc_hooks.c` +
  `tools/clang-mmio-pass`); see [ADR 0003](adr/0003-peripheral-read-capture.md).
- Records the interruption key at every interrupt (`irq_record.c`); see
  [ADR 0004](adr/0004-interrupt-position.md).
- Takes starting-state snapshots (`snapshot.c`); see
  [ADR 0005](adr/0005-starting-state.md).
- Ships the CRC-32 (`bs_crc32`) used for state checksums. The Rust host
  has an identical implementation; tests keep the two in sync.

### Host tools (`host/`, Rust)

- `backstep-core`: recording format, checksums, replay engine.
- `backstep-emu`: the Cortex-M4 emulator ([ADR 0006](adr/0006-emulator.md),
  exceptions in [ADR 0007](adr/0007-exceptions.md)).
- `backstep-replay`: the recording format and the replay engine
  ([ADR 0008](adr/0008-recording-format.md); interrupts and faults in
  [ADR 0009](adr/0009-replaying-interrupts.md)).
- `backstep-gdb`: the GDB remote protocol server over a replay timeline
  (reverse debugging, [ADR 0010](adr/0010-reverse-debugging.md)).
- `backstep-query`: questions answered from the replay: who wrote a
  variable, lost updates, the interrupt and fault timeline; DWARF
  debug information for variables and source lines
  ([ADR 0011](adr/0011-queries.md)).
- `backstep-cli`: the `backstep` command.
- `unsafe` code is forbidden in the workspace (`unsafe_code = "forbid"`).
- The emulator is written from scratch so the whole project stays MIT
  licensed (Unicorn and QEMU are GPL); see
  [ADR 0001](adr/0001-languages-and-layout.md).

## Correctness principle: never show a false history

Replay is only useful if it is exact. Backstep defends this in three ways:

1. **Differential testing** against QEMU, instruction by instruction
   (M2, M3, M4).
2. **State checksums in the recording.** The recorder periodically logs a
   CRC-32 of CPU registers (and, at checkpoints, memory). The replay
   recomputes them; on the first mismatch it stops and reports where the
   divergence was detected.
3. **Hardware cross-checks**: the replay's final state must equal the
   state dumped by the real chip (M3 onwards).

## Firmware targets

| Target | Purpose |
|--------|---------|
| `qemu-mps2-an386` | Deterministic, scriptable reference for CI |
| `nucleo-f411re` | First real board |

QEMU has no STM32F411 model. CI also boots the Nucleo image on QEMU's
`netduinoplus2` (STM32F405), whose RCC, GPIOA, USART2, flash and SRAM
addresses match the F411, to check its register programming before it
reaches hardware. See [ADR 0002](adr/0002-reference-targets.md).
