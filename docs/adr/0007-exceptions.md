# ADR 0007: The emulator's exception model

**Status:** Accepted (M2, part 3)

## Context

Backstep exists to explain crashes, and on a Cortex-M a crash is an
exception: a HardFault, a UsageFault, a stack overflow during exception
entry. Interrupts are exceptions too. The replay emulator must take,
nest, tail-chain and return from exceptions exactly as the chip does,
including the stack frames it writes to memory, or the history it shows
after the first interrupt is wrong.

## Decision

**What is modelled** (`host/crates/backstep-emu/src/cpu/exceptions.rs`,
following the ARMv7-M Architecture Reference Manual, B1.5):

- Exception entry (`PushStack`, `ExceptionTaken`): the 8-word frame on
  the current stack (MSP, or PSP in thread mode with CONTROL.SPSEL),
  8-byte realignment when CCR.STKALIGN is set (xPSR bit 9), `EXC_RETURN`
  in LR, the vector read from VTOR, EPSR.T from the vector's bit 0,
  CONTROL.SPSEL cleared, the exclusive monitor cleared.
- Exception return (`ExceptionReturn`, `PopStack`) from any instruction
  that loads `EXC_RETURN` into the PC in handler mode (BX, POP, LDR,
  LDM): to handler mode, to thread mode on MSP or PSP; FAULTMASK cleared
  (except when returning from NMI).
- Tail-chaining: on return, a pending exception that can preempt the
  context being returned to is entered at once, without unstacking. The
  next handler starts with the previous handler's registers and the same
  frame, as on the chip; a pop followed by a new push would not be the
  same, because r0-r3 and r12 would be reloaded.
- Priorities: fixed (Reset -3, NMI -2, HardFault -1) and configurable
  (SHPR, NVIC IPR, only the implemented bits), group and subpriority from
  AIRCR.PRIGROUP, the execution priority boosted by PRIMASK, BASEPRI and
  FAULTMASK. Of the pending exceptions, the lowest priority value, then
  the lowest exception number, is taken next. It is taken when its group
  priority is higher than the execution priority.
- Faults: UsageFault (UNDEFINSTR, INVSTATE, UNALIGNED including
  CCR.UNALIGN_TRP, DIVBYZERO with CCR.DIV_0_TRP), BusFault (precise data
  errors with BFAR, instruction-fetch errors) and SVC. Each escalates to
  HardFault (HFSR.FORCED) when disabled or not urgent enough, and a fault
  that cannot be escalated is a lockup (`Stop::Lockup`). UNPREDICTABLE
  encodings are treated as UNDEFINED, as the Cortex-M4 does for the
  encodings compilers never emit.
- The System Control Space registers that hold this state: ICSR, VTOR,
  AIRCR, SCR, CCR, SHPR1-3, SHCSR, CFSR, HFSR, MMFAR, BFAR, NVIC
  ISER/ICER/ISPR/ICPR/IABR/IPR and STIR, with their W1C and privilege
  rules. SysTick, DWT, CPUID and the debug registers are not modelled:
  they go to the `Bus`, so replay answers them from the recording.
- `Cpu::set_pending(n)` is how replay raises a recorded interrupt; it
  is taken at the next instruction boundary at which it can preempt.
  `Cpu::exceptions = false` turns the model off ("stop at the fault"
  debugging and unit tests).

**What is not modelled, and stops instead of guessing**
(`Stop::Unsupported`): floating-point frames (M7), faults during
stacking or unstacking (derived faults), invalid exception returns
(UsageFault INVPC, which only broken firmware triggers), SYSRESETREQ, and
MemManage faults (no MPU model yet). Stores to unmapped addresses are
precise BusFaults here and in QEMU; on the chip a buffered store fails
*imprecisely*, later. Replay of a recording does not depend on this
(the fault is in the recording), but it is noted for M4.

**Timing.** The emulator takes an exception at the first instruction
boundary at which it is pending and allowed, as QEMU does. On the chip a
software-pended exception can be taken a few cycles later. During replay
the recorded interrupt positions (ADR 0004) decide where interrupts
happen. Making pending writes and recorded positions agree is M4's job.

## Verification

The QEMU differential test (`scripts/emu-difftest.py`, ADR 0006) covers
exceptions:

- Every program has a vector table and generated handlers. Its setup
  chooses random priorities (sometimes all equal), PRIGROUP, fault
  enables and CCR traps.
- Thread-mode code triggers exceptions in all the supported ways: SVC
  (also inside IT blocks and with the stack misaligned by 4), and PendSV,
  SysTick, NMI and IRQs pended through ICSR, STIR and ISPR. It also pends
  while masked and then unmasks, enables pending IRQs, changes priorities
  at run time, and runs on the process stack. It causes undefined
  instructions, divisions by zero, unaligned LDRD/LDM/LDREX, precise
  BusFaults and INVSTATE.
- Handlers read and modify their stack frame and read SCS registers.
  They pend other exceptions (only "later" ones, so they cannot loop) and
  set FAULTMASK, and they return through BX LR, POP {..., pc} or LDR pc.
  Fault handlers clear CFSR/HFSR, step over the faulting instruction and
  advance the stacked ITSTATE.
- One program in 20 ends in a deliberate lockup, and QEMU and the
  emulator must both lock up at the same instruction.
- Every program ends by writing its data and stack RAM to a file through
  semihosting: QEMU writes its copy, `backstep-emu-trace` implements the
  same calls, and the two must be byte-identical.
- Exception events are counted (`STAT` lines of the trace tool), and the
  run fails unless all 22 kinds happen. They include entries of every
  kind, nesting, tail-chaining, escalation, realignment, process-stack
  frames, every return type, every fault cause and lockup.

QEMU artefacts found and handled:

- After an NVIC write, QEMU logs the state of the next instruction and
  then takes the exception without executing it; `-d exec` reports this
  as "Stopped execution of TB chain", and those states are dropped.
- For a branch with bit 0 clear to a target that is not word-aligned,
  QEMU reports UNALIGNED instead of the architecture's INVSTATE: its
  ARM-state PC check comes first. Generated targets are word-aligned.

## Evidence

- Three fresh runs of 460 programs each (seeds 2-4): 3,190,739
  instructions identical to QEMU, with 61,047 exceptions taken. All 1,311
  memory dumps were byte-identical (the other 69 programs end in their
  deliberate lockup, which matched too). CI runs the same
  1,000,000-instruction check on every change (seed 1).
- Bugs found by the differential test while building this:
  - SHPR and NVIC IPR accesses chose the wrong register bank (an address
    comparison was the wrong way round).
  - Tail-chaining was first implemented as a pop followed by a push,
    which reloads r0-r3 and r12. QEMU, like the chip, keeps the
    handler's values.
- Weaknesses of the test itself, fixed:
  - Exceptions of equal priority were rarely pending together, so the
    exception-number tie-break went unchecked.
  - Nothing cleared FAULTMASK by returning.
  - A looping emulator exhausted memory instead of failing (runs are now
    bounded).
  - The dump file name lacked the NUL terminator QEMU's semihosting
    needs.
- Mutation testing: 18 deliberately introduced bugs in the exception
  model are each caught. They cover:
  - frame realignment and the stacked xPSR bit 9, the stacked r12, and
    the EXC_RETURN value in handler mode
  - tail-chaining, PRIGROUP, BASEPRI, the escalation comparison and the
    tie-break order
  - ICSR.VECTPENDING, SHCSR.SVCALLACT and HFSR.FORCED
  - FAULTMASK on return and ITSTATE on return
  - the SVC return address, process-stack frames, CFSR write-1-to-clear,
    IRQ enables and STIR
- Coverage (`scripts/emu-coverage.sh`, unit tests plus 40 programs):
  93.3% of the lines and 95.4% of the regions of the decoder and exception
  model (`cpu.rs`, `cpu/thumb32.rs`, `cpu/exceptions.rs`), with every
  function executed. CI requires at least 90%.
- Unit tests (`tests/exceptions.rs`) cover:
  - instruction-fetch bus errors, escalation with FORCED, and a taken
    and returned UsageFault
  - SVC's return address and its escalation, and lockup
  - unprivileged SCS access
  - the replay API for raising interrupts (priorities, tail-chaining,
    disabled interrupts) and the exclusive monitor on entry
  - every unsupported case, and DIV_0_TRP

## Consequences

- M3 and M4 replay recordings on this model. Interrupt injection uses
  `set_pending` at the recorded positions.
- The unsupported cases are explicit stops, so replay of firmware that
  hits one says so instead of producing a false history.
