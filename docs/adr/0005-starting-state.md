# ADR 0005: The starting-state snapshot

**Status:** Accepted (M1)

## Context

Replay starts from a known machine state and re-executes the firmware from
there. We must know exactly what that state contains, and prove that
nothing is missing.

## Decision

A snapshot (`recorder/include/backstep_snapshot.h`, format version 1)
contains:

1. **CPU registers** at the snapshot point: r4-r11, SP, LR, the resume PC,
   xPSR flags, MSP, PSP, PRIMASK, BASEPRI, FAULTMASK, CONTROL. The snapshot
   is taken at a function call, so r0-r3 and r12 are scratch and stored as
   zero.
2. **System Control Space state that changes execution**: VTOR, AIRCR
   priority grouping, CCR, SHPR1-3, fault-handler enables, NVIC enables
   and all 240 priorities, MPU regions, CPACR, and the SysTick
   configuration.
3. **RAM regions**: the program's `.data` + `.bss`, and the live stack
   from SP to the top of the stack. Each region is covered by a CRC-32
   computed on the device and verified on the host.

Peripheral registers are **not** part of the snapshot. Replay never models
peripherals; it replays the recorded values of every peripheral read
(ADR 0003).

### Capturing without disturbing the state

`bs_snapshot_take(cpu, save)` works like `setjmp()`. It records the
registers, then calls `save()` on the stack *below* the caller's SP
(memory the program treats as dead) and returns 0. A machine restored from
the snapshot resumes at the same return point with the function returning
1. `save()` may only write memory outside the saved regions. Backstep's
own buffers live in a dedicated `.backstep` linker section placed after
`.bss` for this reason. So the RAM copied out is exactly the RAM at the
snapshot point, which the on-device CRC confirms.

## Evidence

`scripts/snapshot-restore-test.sh` (in CI):

1. Firmware builds state in every place a program keeps it: initialised
   and zeroed globals, a stack array, values held in callee-saved
   registers across the call, exception priorities, an enabled interrupt
   and its priority. Then it takes a snapshot and runs a workload that
   depends on all of it.
2. A **fresh** QEMU, halted at reset, is given only the snapshot file
   through GDB and resumed through `bs_snapshot_resume()`. The workload's
   result is identical.
3. **Negative controls.** Leaving out `.data/.bss`, the stack, the
   registers, or the SCS state each changes the result. Every part of the
   snapshot is necessary, and the test would notice if one were lost.

While building the test we found that QEMU ignores debugger writes to the
System Control Space. That is why restoring goes through a small on-target
routine (`bs_snapshot_resume`), which applies the SCS state and then the
CPU registers. On real hardware, debugger writes over SWD do work, but
keeping one path for both is simpler.

## Consequences and open points

- The replay engine (M3) loads snapshots straight into its emulator and
  parses the same format in Rust (`backstep-replay`, ADR 0008), where the
  snapshot is the `SNAP` chunk of a recording. `scripts/snapshot_tool.py`
  is the prototype reader.
- *Update (M3):* `bs_snapshot_take()` now returns 0 with r1-r3 and r12
  zero and the caller's flags (and GE) restored. The state at the start
  of a recording then depends only on the snapshot, as an
  instruction-for-instruction replay needs.
- **Reads of core peripherals in the PPB** (SysTick's current value, the
  DWT cycle counter) cannot be trapped by the MPU, so ADR 0003 does not
  capture them. M3 must handle them, for example with DWT data watchpoints
  that raise a DebugMonitor exception on those few addresses, or must
  detect them and report the limitation. Until then, firmware that reads
  them replays with a detected divergence. *M3 decision:* replay reports
  such a load as unrecorded and stops (ADR 0008); capturing them with
  watchpoints remains open.
- FPU registers are recorded since M7 (`bs_snapshot_fp`, the FPUS chunk:
  [ADR 0012](0012-floating-point.md)). PSP-based RTOS
  thread stacks are added with FreeRTOS support (M8).
- When a debug probe is attached, a development-time snapshot can also be
  taken by halting the core and reading RAM over SWD, with no firmware
  code at all. Field recording (M9) uses the firmware path above.
