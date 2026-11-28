# ADR 0009: Replaying interrupts and faults

**Status:** Accepted (M4); lookahead and stub-entry chains added in M5

## Context

ADR 0004 records where each interrupt arrived as a key (resume PC,
r0-r12, SP, LR, xPSR, peripheral-read count, and a hash of 128 bytes of
live stack). M3 replays programs without interrupts. M4 must replay
interrupt-driven programs exactly: every interrupt taken before the same
instruction as on the chip, nested ones included, and a fault that ends
the recording (a crash dump).

The first replays of interrupt recordings diverged after a few hundred
interrupts. Every register matched at the recorded position, but the
stack hash did not. The 128 bytes above SP include *dead* slots: frame
padding and locals that the program has not written yet. On the chip they
hold whatever the recorder's own code left there, and that code runs only
on the chip:

- the MPU-trap handler (its exception frame, its locals and the decoder's)
  below the stack of every instruction that touches a peripheral,
- `bs_irq_record`'s locals below each interrupt stub,
- the snapshot's save callback below `bs_record_begin`,
- the stack below SP at the start, which was not in the snapshot.

## Decision

**Replay delivers interrupts by key.** Before each instruction, if the
core's state equals the next recorded key, the emulator takes that
exception now (`Cpu::interrupt_now`: full exception entry, the same as a
hardware interrupt). It repeats this for the following key, because an
interrupt that preempted another one at its stub's first instruction
has the same position. If the core cannot take the interrupt (masked, or
not the most urgent), replay stops with `InterruptNotTaken`. Exceptions
the program raises itself (faults, SVC, PendSV it pends, the escalation
to HardFault) are not recorded: the emulator raises them, as the chip
did.

**The recorder leaves no trace in the program's memory that replay cannot
reproduce.** Replayed memory then equals the chip's everywhere, dead
stack included, and the stack hash is a plain function of the program's
own execution:

| Who runs on the chip | What it leaves | What makes it reproducible |
|---|---|---|
| MPU-trap handler, for each trapped peripheral access | its 8-word exception frame below the program's SP, its own stack below that | The handler's stub zeroes the 520 bytes below its stack top before returning (`BS_TRAP_STACK_BYTES`). Replay, after every instruction that accessed a peripheral at priority ≥ 0, writes the same frame (r0-r3, r12, LR, the next PC and xPSR, with the alignment word and bit 9 as the core does) and the same zeros. |
| `bs_irq_record`, in every stub | its locals below the stub's frame; r0-r3, r12, LR and the flags | The stub zeroes the 128 bytes below its SP (`BS_IRQ_STUB_SCRATCH_BYTES`), clears the flags and r0-r3, r12, then reloads LR before branching to the handler. Replay runs the stub too; there `bs_irq_record` returns at once. |
| `bs_snapshot_take` at the start | its save frame | It zeroes the 1024 bytes below the caller's SP when `save` returns (`BS_SNAPSHOT_SAVE_BYTES`); the snapshot stores those bytes as zeros. |
| the program before the recording | dead stack below SP | The snapshot holds the whole stack area, `[stack_bottom, stack_top)`, not only the live part. |

Each zeroed size must cover the code's deepest stack.
`scripts/check-stack-usage.py` (CI) proves it from GCC's call graph and
stack usage at -O0, -O2 and -Os (deepest: 232 of 520, 80 of 128 and 136
of 1024 bytes, all at -O0), and that the stubs' assembly and replay
zero the sizes the macros give. The stubs store with `stmdb` of several
zeroed registers, about 45 instructions per trapped access and 15 per
interrupt.

**Only what the MPU traps is recorded.** At execution priority -1 or -2
(HardFault, NMI, FAULTMASK set) the MPU is bypassed, so a peripheral load
there was not recorded. Replay reports it (`UnrecordedLoad`) instead of
matching it against the next recorded load.

**Lookahead (M5).** A key can match at several positions: in a loop
whose registers, flags and stack repeat while only static RAM changes,
every iteration has the same key (ADR 0004). Replay delivers an
interrupt at the first match. If the replay then diverges (a load, a
later key, or the end state), it goes back to just before the most
recent placement and delivers that interrupt where its key matches next;
if there is no later match, it goes back to the placement before, and so
on. The engine keeps the machine before each of the last 64 placements
for this; the emulator's memory is copy-on-write (1 KiB pages in 64 KiB
blocks), so a copy costs a few hundred bytes. The search for a later
match stops after twice the longest gap between interrupts seen (at
least 2 M instructions), and all retries together after 50 M
instructions or four times the first attempt; then replay reports the
first attempt's divergence. `backstep replay` says how many placements
it retried. A recording replayed this way still passes every check of
ADR 0008, so a wrong choice cannot be reported as a success.

**Interrupts that preempted another before its stub ran** are logged
innermost first: the inner stub runs `bs_irq_record` before the outer
one. Replay recognises such a chain (each event's xPSR names the next
event's exception, with the same read count) when the next event's key
does not match but a later one in the chain does, and delivers the
chain outermost first, checking each inner key at the stub entry of the
one it preempted. When the outer interrupt was tail-chained from the
capture handler, its stub began with the capture's scratch registers
(r0 = its stack top - 32, r1-r3 and r12 zero, Z and C set); replay sets
them when the inner key says so, since the inner exception stacks them.

**Interrupts and the recording's edges.** `bs_record_begin` masks
interrupts from enabling the capture until after the snapshot, and
`bs_record_end` masks them before the end snapshot. So every recorded
interrupt lies inside the recorded stretch, and the stretch's first and
last states are not interrupted. The IRQS chunk (ADR 0008) carries the
events, the number dropped, and the stack top the hash is clamped to.

## Evidence

`scripts/replay-irq-test.py` (CI, firmware job) uses
`replay_irq_test.c`: SysTick (priority 0x40), TIMER0 (0x80) and TIMER1
(0xC0) through `BS_IRQ_STUB`, each handler reading its timer and picking
a random reload. The main code runs checksums, a sort, recursion,
peripheral polling and spins on flags set by the handlers.

| Check | Result |
|---|---|
| Seeds 1-100, 400+ interrupts each: replay exact (every load, every interrupt at its key, end registers and RAM CRC) | **100 of 100**: 42,028 interrupts, 1,826 nested, 250.4 M instructions |
| All three sources, and nesting | seen |
| Crash runs (a handler corrupts a function pointer; the HardFault ends the recording), 10 seeds | **10 of 10** |
| Ambiguous keys (`a=<seed>`), 20 seeds: replay exact by lookahead (M5) | **20 of 20**, 76,260 placements retried |
| QEMU's trace vs the replay's, state for state (r0-r15, xPSR), handlers included, capture code left out; 4 seeds, a crash and an ambiguous run | identical, 1.68 M states, 80 k of them in handlers |
| Byte flips in the IRQS chunk | 200 of 200 rejected |
| Re-signed IRQS edits: PC, r5, read count, stack hash, exception number of the first, middle and last interrupt; one missing, one extra; stack top; dropped count | 19 of 19 detected |

Interrupts also arrive tail-chained from the capture handler: the
chip then enters the handler with the capture's r0-r3 and flags, replay
with the program's; the stub overwrites both before the handler runs, and
its recorded key comes from the frame, which holds the program's.

The test was shown to fail: with replay's trap-frame writing turned off,
3 of 5 seeds diverge, and the crash recordings fail their end RAM
check.

## Consequences

- **Lookahead has limits.** It finds the right match when a wrong one
  shows as a divergence within the next 64 placements and the budget.
  `replay_irq_test.c`'s ambiguous mode (`a=<seed>`: a wait loop whose
  iterations share one key, and one timer read per iteration afterwards)
  is in CI: 20 of 20 recordings replay exactly, after thousands of
  retries, and QEMU's trace of one equals the replay's state for state.
  A wrong match that changes nothing the recording checks (no load, no
  later key, not the end RAM) is replayed as if it were right; it is then
  indistinguishable from the right one by everything recorded.
- The trap-frame emulation encodes what the capture handler does on the
  chip. If that handler changes (its frame, or how much stack it uses),
  `leave_trap_frame` in `backstep-replay` must change with it. The
  trace comparison and the 100 runs catch a mismatch.
- Programs must not use the FPU while recording: the emulator has no FPU
  yet (M7), and an extended exception frame is not modelled.
- Interrupts on the process stack (PSP) are handled in the trap frame,
  but the key's hash and the tests cover the main stack only (ADR 0004).
  RTOS threads come in M8.
- The hardware criterion (20 of 20 runs on the NUCLEO-F411RE) is the
  same test on the board: `record_irq_hw.c` (SysTick, TIM3, TIM4 and a
  crash run) and `scripts/hil/m4_nucleo_f411re.py`; see
  `docs/HARDWARE_VERIFICATION.md`. In CI the image runs on QEMU's
  netduinoplus2, where all 21 recordings replay exactly; QEMU's STM32
  timer model schedules each period later the longer the machine has
  run, so TIM3 and TIM4 fire mostly in the first run there.
