# Asking a recording questions

Two commands answer questions about a recording without a debugger
session. Both first check the recording, as `backstep replay` does. Add
`--json` for output that scripts and IDEs can read.

## `backstep why`: who wrote this variable?

```console
$ backstep why demo_overrun.elf recording.bsr g_rx.limit
g_rx.limit (volatile uint32_t, 4 bytes at 0x20000014) at position 38778: 0x00000065
last written:
  position     34742  write by IRQ8 handler
                      0x000001da in rx_isr at demo_overrun.c:78
                      0x00000064 -> 0x00000065
1 write and 4 reads in the recording up to there
```

The variable is named as in C:

- a global or static variable's name, followed by any number of
  `.member` and `[index]`, for example `g_rx.limit` or
  `g_task.stack[3]`;
- or a memory range written as `0xADDRESS:SIZE`.

The answer is about the end of the recording. With `--at <position>`, it
is about that position instead (positions are what GDB's
`monitor position` shows).

It reports:

- **The last write:** the instruction, its source line and function, and
  whether the main code or which interrupt handler ran it. It also gives
  the value before and after the write.
- **Every write before it** (in the JSON output) and the number of
  reads.
- **Stack overflow:** a write through the stack pointer is flagged. For a
  global variable, that means the stack grew into it:

  ```text
    position      3621  write by main code (thread mode)
                        0x00000090 in walk at demo_stack.c:57
                        0x00005afe -> 0x000000cf
                        through the stack (SP 0x20000088): the stack has grown into it (stack overflow)
  ```

- **Lost updates:** a race between an interrupt and the code it
  interrupted. The code read the variable, the interrupt changed it, and
  the code then wrote back a value based on its stale read:

  ```text
  lost update: a write based on a stale read
    read:
    position      1591  read  by main code (thread mode)
                        0x0000011e in add_main_event at demo_race.c:75
    overwritten writes of another context:
    position      1661  write by IRQ8 handler
                        0x00000274 in tick_isr at demo_race.c:62
                        0x00000008 -> 0x00000009
    the write that overwrote them:
    position      1747  write by main code (thread mode)
                        0x0000013a in add_main_event at demo_race.c:79
                        0x00000009 -> 0x00000009
  ```

All three examples are in `firmware/targets/qemu-mps2-an386/`
(`demo_overrun.c`, `demo_stack.c`, `demo_race.c`). CI checks the answers
for them (`scripts/why-test.py`).

## `backstep timeline`: interrupts and faults

```console
$ backstep timeline replay_irq_test.elf recording.bsr
         0  start               0x00000d60 in bs_record_begin at record.c:134
         2  -> SysTick  from 0x00000d64 in bs_record_begin at record.c:138
        76  <- SysTick
        76  -> IRQ8  from 0x00000d64 in bs_record_begin at record.c:138
       319  <- IRQ8
       319  -> IRQ9  from 0x00000d64 in bs_record_begin at record.c:138
       570  <- IRQ9
       ...
    313919  -> SysTick  from 0x00000806 in wait_flag at replay_irq_test.c:181
    313993  <- SysTick
    314008  -> HardFault (FAULT, invstate)  at 0x00001000 in bs_snapshot_header_build at snapshot.c:107, after 0x00000828 in round_of_work at replay_irq_test.c:234
    314054  end                 0x00000ed0 in bs_snapshot_take at snapshot.c:21
```

Each exception entry shows:

- **The exception:** its name (an interrupt `IRQn`, `SysTick`, a fault,
  `SVCall`, `PendSV`).
- **What kind of entry it was:** for a fault, the causes the core
  recorded (`invstate`, `undefinstr`, `nocp`, `unaligned`, `divbyzero`,
  `ibuserr`, ...); whether it was tail-chained or nested.
- **Where it came from:** the instruction it interrupted, or the one
  that faulted. For a fault, it also shows the instruction executed just
  before. Above, that is the call through a corrupted function pointer
  whose target, 0x1000, is not Thumb code.

Returns are shown too, and nesting is indented. With `--json` you get one
object per event, with the position and the source location.

How it works: [ADR 0011](adr/0011-queries.md).
