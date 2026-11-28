# Hardware verification

Automated tests run in CI on every pull request. A few acceptance
criteria can only be confirmed on a real board; they are tracked here.
Each entry has a script that prints a report block to paste below.

**Board:** ST NUCLEO-F411RE (STM32F411RE).

| Milestone | Check | Script | Status |
|-----------|-------|--------|--------|
| M0 | Blinky boots from flash, LED LD2 blinks at 1 Hz, console at 115200 baud, heartbeat period 1.000 s, on-target CRC-32 correct | `scripts/hil/m0_nucleo_f411re.py` | Pending |
| M1 | MPU trap on real STM32F411 registers: all load forms, stores, interrupts at priority 0x80 and 0, trapped console; cycles per trapped load (DWT) | `scripts/hil/m1_nucleo_f411re.py` | Pending |
| M2 | Exclusive monitor: LDREX/STREX and CLREX behave as required; measure STREX to another address, of another size, after a plain store and after an exception | `scripts/hil/m2_nucleo_f411re.py` | Pending |

## M0: blinky on NUCLEO-F411RE

1. Build the firmware (or download the `firmware-<commit>` artifact from
   the CI run):
   ```bash
   make -C firmware
   ```
2. Plug in the Nucleo with a USB cable. A USB drive named `NODE_F411RE` appears.
3. Copy `firmware/build/nucleo-f411re/blinky.bin` onto that drive. The
   ST-LINK flashes it and resets the board. (Alternatively:
   `probe-rs download --chip STM32F411RETx firmware/build/nucleo-f411re/blinky.elf`.)
4. Install pyserial once: `pip install pyserial`.
5. Run the test with the board's serial port:
   ```bash
   python3 scripts/hil/m0_nucleo_f411re.py /dev/ttyACM0   # Linux
   python3 scripts/hil/m0_nucleo_f411re.py /dev/cu.usbmodem1234   # macOS
   python3 scripts/hil/m0_nucleo_f411re.py COM5           # Windows
   ```
6. Press the black RESET button when asked, answer the LED question, and
   paste the report block here.

**Report:** _not run yet_

## M1: MPU trap on NUCLEO-F411RE

Same procedure as M0, with a different image and script:

1. `make -C firmware`
2. Copy `firmware/build/nucleo-f411re/mmio_trap_hw.bin` onto the
   `NODE_F411RE` drive.
3. `python3 scripts/hil/m1_nucleo_f411re.py <serial-port>`
4. Press RESET when asked and paste the report block here. The measured
   "extra cycles per trapped load" also goes into
   [ADR 0003](adr/0003-peripheral-read-capture.md).

The test temporarily writes `GPIOB_AFRH` (PB8-PB15 alternate functions,
unused while those pins are inputs after reset) and restores it.

**Report:** _not run yet_

## M2: exclusive monitor on NUCLEO-F411RE

Same procedure, with `firmware/build/nucleo-f411re/exclusive_hw.bin` and
`scripts/hil/m2_nucleo_f411re.py <serial-port>`. The report includes a
table of the chip's STREX results next to the emulator's defaults and says
which emulator setting matches this chip; that result goes into
[ADR 0006](adr/0006-emulator.md) and decides the replay default for
STM32F4 recordings (M3).

QEMU's results for reference (STM32F405 model, printed by CI):
`other_address` fails, `other_size` succeeds, `plain_store` succeeds,
`exception` fails.

**Report:** _not run yet_
