# ADR 0011: "Who wrote this?" queries

**Status:** Accepted (M6)

## Context

With reverse debugging (ADR 0010), finding the code that corrupted a
variable is one watchpoint and one `reverse-continue` away, if you know
GDB well. M6 asks for the same answer without a debugger session, as a
command and as JSON for scripts and IDEs:

- `backstep why <variable>`: the code that last wrote the variable;
- an interrupt and fault timeline;
- for each of three common bug kinds (stack overflow, buffer overrun,
  interrupt/task race), automated tests that the answer points at the
  right source line.

## Decision

**A query crate on the timeline** (`backstep-query`). Answers come from
the exact replay, not from a heuristic.

**`why`** replays the recording from the start up to the position asked
about (the end by default), with an access watchpoint on the variable.
It records every read and write:

- the instruction's position, address and source location;
- the context it ran in (main code, or which exception handler);
- the stack pointer;
- the variable's value before and after each write.

The answer gives:

- **The last write**, and every write before it, oldest first.
- **Lost updates.** A context reads the variable, and another context
  writes it before the first one writes it back without reading it again.
  This is the read-modify-write race between an interrupt and the code it
  interrupted.
- **Stack overflow.** A write within 64 bytes of the writer's stack
  pointer is flagged. For a global variable, that means the stack has
  grown into it.

**The variable** is named as in C: a global's name, then any number of
`.member` and `[index]` (`g_rx.limit`, `g_task.stack[3]`). It can also be
given as `0xADDRESS:SIZE`. Names, addresses and types come from the
ELF's DWARF information, read with `gimli`:

- base types, typedefs and qualifiers;
- pointers;
- structs and unions with member offsets;
- arrays with bounds checks.

Without debug information, the symbol table gives the address, and
4 bytes are assumed.

**Source locations** come from the DWARF line tables, with two
corrections:

- **Discarded code is ignored.** `--gc-sections` leaves line-table
  sequences for discarded functions at address 0. On a target with code
  at low addresses (QEMU's mps2), these overlapped the real code and
  named the wrong function in the first test. Sequences that start at
  address 0 are therefore ignored: no function starts there, because the
  vector table does.
- **Function names come from the symbol table.** Inlined function names
  from `addr2line` are used only when the outermost frame agrees with
  the symbol table.

**`timeline`** replays the whole recording and reports each exception
entry and return:

- **Each entry:** the exception's number and name, whether it is a fault
  and the fault causes the core recorded (CFSR bits), and whether it was
  tail-chained or nested.
- **Where it came from:** the stacked return address, which is the
  instruction that was interrupted or that faulted.

It detects them from the emulator's exception statistics when IPSR
changes, so the cost per instruction is one comparison.

Both commands print text, or JSON with `--json`, for scripts and IDEs.

## Evidence

`scripts/why-test.py` (CI) records three example bugs on QEMU. Each
firmware marks the lines the answer must point at with a comment:

| Example | What `why` must say | Result |
|---|---|---|
| `demo_overrun.c`: an off-by-one interrupt handler | the last write to `g_rx.limit` is the overrunning store (`DEMO-BUG-STORE`), in the IRQ8 handler, 100 before | pass |
| `demo_stack.c`: a task on its own PSP stack recurses too deep | the last write to `g_task.state` is the push in `walk()`'s prologue (`WHY-STACK-PUSH`), in thread mode, flagged as through the stack pointer | pass |
| `demo_race.c`: main code and interrupt both count in `g_events` | a lost update: the read at `WHY-RACE-READ`, the handler's write at `WHY-RACE-ISR`, overwritten at `WHY-RACE-STORE` | pass |

The same script checks that:

- the timeline of an interrupt recording lists every recorded
  interrupt, flags nested ones, and keeps positions in order;
- a crash recording shows one HardFault with the cause INVSTATE, at the
  bad call target;
- `--at` works, as do an untouched variable, a raw address, and member
  and element lookup;
- an unknown name, a member of a non-struct and an index past the end
  are errors (exit 2).

Rust tests cover:

- the lost-update rule: a stale read counts; reading again, a blind write
  or a single context does not;
- `why` and the timeline on the checked-in fixture recording, whose ELF
  has no debug information: the symbol fallback, the values chaining
  from write to write, every recorded interrupt in the timeline, and the
  crash.

## Consequences

- **Cost:** a query replays the recording once (and once more to check
  it), at about 12 M instructions/s.
- **Lost updates are reported per variable.** A race through two
  variables, or a check-then-act race, needs the write history (`writes`)
  and a person to read it.
- **Local variables are not supported** (their location depends on the
  position). Globals and statics are, which covers the memory-corruption
  bugs this tool is for. In GDB, every variable can be watched.
- **The stack-overflow flag is a heuristic:** a write within 64 bytes of
  SP. It is meaningful for globals, not for variables on the stack.
