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
| M3 | Record polling code reading real STM32F411 registers, send the recording over the UART, replay it on the PC: every load reproduced, end registers and RAM checksum equal | `scripts/hil/m3_nucleo_f411re.py` | Pending |
| M4 | Nested interrupts from 3 sources (SysTick, TIM3, TIM4) at random rates recorded on the board: 20 of 20 runs replay exactly on the PC, plus a run ended by a HardFault | `scripts/hil/m4_nucleo_f411re.py` | Pending |
| M7 | Floating point on the STM32F411's FPU gives the emulator's results and FPSCR flags, including the cases where the emulator follows QEMU (listed below) | The image and script come with FPU recording (M7) | Pending |

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

## M3: record on NUCLEO-F411RE, replay on the PC

1. `make -C firmware` and `(cd host && cargo build --release)`
2. Copy `firmware/build/nucleo-f411re/record_hw.bin` onto the
   `NODE_F411RE` drive.
3. `python3 scripts/hil/m3_nucleo_f411re.py <serial-port>`
4. Press RESET when asked. The board records TIM2 counter, GPIO input,
   USART status and RCC reads (the timer gives different values every
   run), then sends the recording as hex. The script saves it as
   `recording.bsr` and runs `backstep replay`, which must reproduce every
   load and the end registers and RAM checksum the board recorded. Paste
   the report block here.

Pressing or holding the blue user button during the run is fine (PC13
is read and changes the path taken). The same image and script run in
CI on QEMU's netduinoplus2 (`scripts/qemu-nucleo-replay-smoke.py`).
This check depends on the MPU trap working on the board (M1 check above).

**Report:** _not run yet_

## M4: interrupts recorded on NUCLEO-F411RE, replayed on the PC

1. `make -C firmware` and `(cd host && cargo build --release)`
2. Copy `firmware/build/nucleo-f411re/record_irq_hw.bin` onto the
   `NODE_F411RE` drive.
3. `python3 scripts/hil/m4_nucleo_f411re.py <serial-port>`
4. Press RESET when asked. The board records 20 runs in which SysTick
   (priority 0x40), TIM3 (0x80) and TIM4 (0xC0) interrupt the main code
   at random rates and nest, then a 21st run that the SysTick handler
   crashes on purpose (a corrupted function pointer; the HardFault ends
   the recording). It sends every recording as hex; this takes about two
   minutes. The script saves them as `run-<n>.bsr` and replays each with
   `backstep replay`. Every one must reproduce every interrupt at its
   recorded position, every load, and the end registers and RAM checksum.
   Each of the 20 runs must have been interrupted by all three sources,
   and some interrupts must have nested. Paste the report block here.

The same image and script run in CI on QEMU's netduinoplus2
(`scripts/qemu-nucleo-irq-smoke.py`). QEMU's STM32 timer model fires
TIM3 and TIM4 mostly in the first run, so there the three sources are
required over all runs together; the board must show them in every run.
This check depends on the MPU trap (M1) and the M3 replay working on the
board.

**Report:** _not run yet_

## M7: floating point on NUCLEO-F411RE

The emulator's floating-point unit is checked against QEMU on 1,000,000
instructions in CI ([ADR 0012](adr/0012-floating-point.md)). Where the
architecture leaves room, the emulator does what QEMU does; these are
the cases the board must confirm (the test image and script come
together with recording firmware that uses the FPU, later in M7):

- A new floating-point context (FPCCR.ASPEN set, CONTROL.FPCA clear)
  sets all of FPSCR from FPDSCR: NZCV and the exception flags become 0.
- VCVTB/VCVTT to half precision with FPSCR.AHP: a NaN becomes a zero
  with the NaN's sign.
- `VMOV.32 Dd[x], Rt` and `VMOV.32 Rt, Dd[x]` execute (the GNU assembler
  does not accept them for FPv4-SP).
- VCVT from fixed point rounds to nearest whatever FPSCR.RMode says.
- After an exception entry that stores the extended frame at once
  (FPCCR.LSPEN clear), S0-S15 and FPSCR read as zero in the handler.
- FPCCR.UFRDY (bit 10) is set at a lazy exception entry when a
  UsageFault could have preempted the interrupted code.
- An FP instruction that faults (say an unaligned VLDR) has still set
  CONTROL.FPCA (and done lazy preservation): the fault's frame is an
  extended one.

**Report:** _not run yet_
