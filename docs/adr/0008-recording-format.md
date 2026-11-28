# ADR 0008: The recording file and how replay checks it

**Status:** Accepted (M3, part 1)

## Context

A recording must let `backstep replay` re-run a stretch of the program
exactly, on the PC, with nothing but the firmware ELF. It must also make
a wrong replay impossible to miss: a recording that is corrupted, belongs
to other firmware, or was made by a buggy recorder must be reported, not
replayed into a plausible-looking false history.

## Decision

**What is recorded** (`recorder/include/backstep_record.h`):

- `bs_record_begin()` turns on peripheral-read capture (the MPU trap,
  ADR 0003) and takes the starting-state snapshot (ADR 0005). The
  recording starts where it returns.
- Every peripheral load in between is logged: address, size, value, and
  the address of the load instruction.
- `bs_record_end()` captures the end state with a second
  `bs_snapshot_take()`. Its `save` callback runs below the caller's stack,
  turns capture off and computes a CRC-32 of the program's static RAM and
  live stack. Replay stops at the entry of that call: the first time it
  reaches `bs_snapshot_take`.
- The recorder's own state, including the capture modules' counters and
  logs, is in the `.backstep` section, outside the RAM that is snapshotted
  and checksummed (`BS_STATE`). Replay never runs the capture code, so
  this state would otherwise differ. The startup code zeroes `.backstep`.
- `bs_snapshot_take()` returns with r1-r3 and r12 zero and the flags as
  they were at the call. The registers at the start of a recording then
  depend only on the snapshot, which an instruction-for-instruction
  comparison needs.

**The file** (little-endian `u32`s):

```text
header   "BSRC", version 1, header size 16, chunk count 4
chunk    type (4 ASCII bytes), payload length, payload, CRC-32 of the payload
  TGTD   target id, CRC-32 of the firmware image, image start, image length
  SNAP   the snapshot exactly as ADR 0005 defines it (header with its own
         region CRC, then the RAM bytes)
  MMIO   load count, dropped count, then 16 bytes per load
         (address, value, pc, size)
  END    end registers (struct bs_cpu_state), RAM CRC-32, region count,
         the regions checksummed
trailer  CRC-32 of every byte before it
```

The firmware image is the flash contents from the vector table to the
end of the `.data` initialisers (`__backstep_image_start/end`, from the
linker scripts). Replay computes the same CRC from the ELF. Chunks are
fixed in number and order in version 1; later versions (M4: interrupt
positions) add chunk types and bump the version.

**The self-check** has two layers:

1. *Integrity.* The trailer and per-chunk CRCs detect any change to the
   bytes of the file, and so does the snapshot's own region CRC. So does
   any truncation, extension, wrong magic or version, and a recorder that
   dropped loads (`dropped != 0`). `backstep inspect` checks this layer
   only.
2. *Consistency.* During replay every peripheral load must be the next
   recorded one: the same instruction, address and size. When replay
   reaches the end point, every recorded load must have been used, the
   registers the recorder captures must match, and so must the RAM
   CRC-32. Then come r4-r11, SP, LR, the flags and GE, MSP, PSP, PRIMASK,
   BASEPRI, FAULTMASK and CONTROL. The firmware CRC and the target must
   match the ELF before replay starts.

A corruption that passes layer 1 (re-signed, or a recorder bug) is caught
by layer 2 whenever it changes the execution or the end state.
Corruptions of state the program never uses again cannot change the
replay, so layer 2 cannot see them; layer 1 catches them.

**Exit status of `backstep replay`:** 0 reproduced, 1 diverged (with the
instruction and the access), 2 unusable input (corrupted file, other
firmware, unknown target).

**Limits (M3):** no interrupts during a recording (M4 adds the interrupt
log of ADR 0004). Loads from the Private Peripheral Bus (SysTick, DWT)
are not captured, so replay stops at them and says so. The MPU is not
modelled in replay; the recording's MPU state is the capture's own.

## Verification

`scripts/replay-test.py` (CI, firmware job):

- Ten polling workloads (`replay_test.c`) are recorded on QEMU. Between
  them they cover the polled UART, busy-waits on free-running timers,
  data-dependent sorting seeded by a counter, every load size and
  signedness, value-dependent branches and recursion, the dual timer,
  write-and-read-back registers, slow clocks, conditional loads in IT
  blocks, and writeback and register-offset forms. Every value read is
  folded into a global, so a wrong value changes the end RAM.
- Each recording must replay with exit 0. QEMU's own trace of the recorded
  stretch, without the capture handler's states, must equal the replay's
  trace state for state: r0-r15 and xPSR before every instruction.
- 414 byte flips (every header byte, and a sample through the file) and a
  truncation must all be rejected by the checksums. Every single-bit flip
  and every truncation length of a synthetic recording is rejected in the
  unit tests (`host/crates/backstep-replay/tests/format.rs`).
- 42 re-signed corruptions must each be caught by replay. They change the
  value, address and instruction of the first, middle and last loads; drop
  or add a load; change the starting SP or PC, a RAM byte of the
  snapshot, the end RAM CRC or end registers; and change the firmware or
  target identity.

Writing the tests found four problems, all fixed:

- The capture modules kept their state in `.bss`, so the end RAM never
  matched.
- The reserved bytes of logged loads were never written (random RAM on a
  chip).
- Workloads discarded some values: a timer value, and the values that only
  ended a polling loop. Corrupting them could not be seen, depending on
  the run's timing. Every value read is folded now.
- Parsing now also rejects a recorded value wider than its load (layer 1).

## Consequences

- The hardware criterion of M3 is the same check on the NUCLEO-F411RE:
  the recording goes out over the UART, and the replay's end registers
  and RAM CRC must equal the recorded ones (M3, part 2).
- M4 adds the interrupt log and replays interrupts with
  `Cpu::set_pending` at the recorded positions.
