# Timeline test fixtures

Recordings made on QEMU (`mps2-an386`) with
`firmware/targets/qemu-mps2-an386/replay_irq_test.c`, and that firmware
with its debug sections stripped (`arm-none-eabi-strip --strip-debug`;
the symbol table stays). They let the timeline tests run without QEMU or
an ARM toolchain, on every CI platform.

| File | Command line | What it has |
|------|--------------|-------------|
| `irq_ambiguous.bsr` | `a=1:n=24` | 236,432 instructions, 94 interrupts from 3 sources, nested, with keys that need lookahead |
| `irq_crash.bsr` | `c=2:n=16` | an interrupt recording ended by a HardFault |

To remake them after a change to the recorder or the firmware:

```bash
make -C firmware
cd /tmp && for a in a=1:n=24 c=2:n=16; do
  qemu-system-arm -machine mps2-an386 -cpu cortex-m4 -nographic -monitor none \
    -serial null -semihosting-config enable=on,target=native,arg=$a -icount shift=0 \
    -kernel <repo>/firmware/build/qemu-mps2-an386/replay_irq_test.elf
  # then copy recording.bsr to irq_ambiguous.bsr / irq_crash.bsr
done
arm-none-eabi-strip --strip-debug -o irq_ambiguous.elf <repo>/firmware/build/qemu-mps2-an386/replay_irq_test.elf
```

Both recordings belong to `irq_ambiguous.elf`: replay checks the
firmware's CRC.
