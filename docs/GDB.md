# Debugging a recording with GDB

`backstep gdb` serves a recording to GDB as if it were a running
Cortex-M. You can step, continue, set breakpoints and watchpoints, and
also run **backwards**. The firmware ran once, on the chip or in QEMU;
debugging replays that run, so every value you see is the value it had.

## Start

You need the firmware ELF that was recorded (with debug information) and
the recording (`.bsr`). `backstep gdb` first checks the recording, the
same way `backstep replay` does, then waits for GDB:

```console
$ backstep gdb firmware.elf recording.bsr
backstep gdb: recording checked: 236432 instructions, 94 interrupts
backstep gdb: waiting for GDB on 127.0.0.1:3333 (in GDB: target remote localhost:3333)
```

```console
$ gdb-multiarch firmware.elf          # or arm-none-eabi-gdb
(gdb) target remote localhost:3333
```

GDB can also start the server itself, over a pipe:

```text
(gdb) target remote | backstep gdb --stdio firmware.elf recording.bsr
```

The session starts at the beginning of the recording: where
`bs_record_begin()` returned.

## Commands

All of GDB's usual commands work:

| Forwards | Backwards | |
|---|---|---|
| `stepi`, `nexti` | `reverse-stepi`, `reverse-nexti` | one instruction |
| `step`, `next` | `reverse-step`, `reverse-next` | one source line |
| `finish` | `reverse-finish` | out of the current function |
| `continue` | `reverse-continue` | to a breakpoint or watchpoint, or to the end |

- **Breakpoints:** `break`, `tbreak`, `advance` and `until` work as on a
  live target.
- **Watchpoints:** `watch`, `rwatch` and `awatch` stop on the program's
  own accesses, including the registers an interrupt pushes onto the
  stack.
- **The ends of the recording:** at the end and at the start, GDB says
  "No more reverse-execution history".
- **Interrupts:** interrupts appear where they arrived on the chip. In a
  backtrace, an interrupt handler's caller is shown as
  `<signal handler called>`.
- **Writing:** memory and registers can be read but not written, because
  the recording is history.

Backstep adds two `monitor` commands:

| Command | What it does |
|---|---|
| `monitor position` | The current position (instructions since the start), the last position, and how many interrupts have been delivered |
| `monitor goto <n>` / `start` / `end` | Jump to a position. Then run `maintenance flush register-cache` so GDB rereads the registers. |

## Finding who corrupted a variable

`firmware/targets/qemu-mps2-an386/demo_overrun.c` has an interrupt
handler whose buffer bound is off by one. The extra byte lands in the
setting stored after the buffer. Later, the main code notices the bad
setting and stops recording, as a crash dump would.

```text
(gdb) continue
No more reverse-execution history.
bs_snapshot_take (...) at ../recorder/src/snapshot.c:21   <- the end, called from fail()
(gdb) print g_rx.limit
$1 = 101                                                  <- should be 100
(gdb) watch g_rx.limit
Hardware watchpoint 1: g_rx.limit
(gdb) reverse-continue
Hardware watchpoint 1: g_rx.limit

Old value = 101
New value = 100
0x000001da in rx_isr () at targets/qemu-mps2-an386/demo_overrun.c:78
78	        g_rx.line[g_rx_len] = c;        /* DEMO-BUG-STORE */
(gdb) bt
#0  0x000001da in rx_isr () at targets/qemu-mps2-an386/demo_overrun.c:78
#1  <signal handler called>
#2  take_line (round=3) at targets/qemu-mps2-an386/demo_overrun.c:132
#3  main () at targets/qemu-mps2-an386/demo_overrun.c:173
```

Reading backwards, "Old value" is the value after the store (101) and
"New value" the value before it (100). GDB stops on the store instruction
itself. To try this yourself:

```bash
make -C firmware && (cd host && cargo build --release)
cd /tmp && qemu-system-arm -machine mps2-an386 -cpu cortex-m4 -nographic \
  -semihosting-config enable=on,target=native -icount shift=0 \
  -kernel <repo>/firmware/build/qemu-mps2-an386/demo_overrun.elf
<repo>/host/target/release/backstep gdb \
  <repo>/firmware/build/qemu-mps2-an386/demo_overrun.elf recording.bsr
```

## Limits

- **Ctrl-C:** it stops a long `continue` over TCP, but not over `--stdio`.
- **Positions, not time:** a position counts instructions. It is not
  wall-clock time.
- **Optimised code:** stepping by source line follows GDB's line tables,
  as on a live target.

How it works: [ADR 0010](adr/0010-reverse-debugging.md).
