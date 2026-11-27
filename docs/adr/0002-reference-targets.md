# ADR 0002: Reference targets for testing

**Status:** Accepted (M0)

## Context

Backstep must be tested automatically on every change, and must also work
on real boards. The first real board is a NUCLEO-F411RE (STM32F411RE,
Cortex-M4F). QEMU 8.2 has no STM32F411 model.

## Decision

1. **QEMU `mps2-an386` (Cortex-M4) is the reference target for automated
   tests.** It is Arm's own reference platform, is fully deterministic
   under QEMU, has a simple UART, and supports semihosting so test
   firmware can report PASS/FAIL through QEMU's exit code. Later, QEMU's
   instruction trace on this machine is the oracle for the emulator and
   replay engine.
2. **The NUCLEO-F411RE image is also booted on QEMU `netduinoplus2`
   (STM32F405) in CI.** Its flash, SRAM, RCC, GPIOA and USART2 addresses
   match the F411. QEMU logs every access to the unimplemented RCC and
   GPIO blocks, which lets CI check that the firmware programs the exact
   register values the F411 needs. Timing and the physical LED are checked
   on real hardware (`scripts/hil/`).
3. **Firmware is built soft-float until milestone M7**, when the emulator
   gains floating-point support. The F411's FPU is simply left disabled
   until then.
4. **The Nucleo firmware runs on the 16 MHz HSI reset clock.** No clock
   tree configuration means fewer ways to fail while bringing up the
   board; performance is irrelevant for these tests.

## Consequences

- CI needs only `qemu-system-arm` and `gcc-arm-none-eabi` from Ubuntu.
- STM32-specific peripheral behaviour is never assumed from QEMU; only
  the register values the firmware *writes* are checked there.
