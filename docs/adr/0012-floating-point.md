# ADR 0012: The emulator's floating-point unit

**Status:** Accepted (M7, part 1)

## Context

Real STM32 firmware uses the Cortex-M4's floating-point unit (FPv4-SP):
STM32CubeIDE builds with `-mfloat-abi=hard -mfpu=fpv4-sp-d16` by
default, and the HAL's `SystemInit` turns the unit on. Until now the
emulator stopped at the first floating-point instruction, so such
firmware could not be replayed. Replay must reproduce every result bit
for bit: a value one unit in the last place off, or one exception flag
missing from FPSCR, changes the history the user is shown.

## Decision

**Arithmetic** (`host/crates/backstep-emu/src/fpu.rs`) is our own
single-precision implementation of the Arm pseudocode (`FPAdd`, `FPMul`,
`FPMulAdd`, `FPDiv`, `FPSqrt`, `FPCompare`, `FPToFixed`, `FixedToFP`, the
half-precision conversions, `VFPExpandImm`). It works on bit patterns
with integer arithmetic (u128 significands, exact until the single
rounding step), so results do not depend on the host's floating point.
It implements:

- all four rounding modes; tininess detected before rounding; underflow
  raised only for an inexact tiny result;
- flush-to-zero (FZ): denormal inputs become zero (IDC), tiny results
  become zero (UFC, no IXC); half-precision values are never flushed;
- default NaN (DN), and otherwise the Arm NaN rules: the first
  signalling NaN, else the first quiet NaN, in operand order (for fused
  multiply-add: addend first), quieted; `inf * 0 + quiet NaN` gives the
  default NaN and IOC;
- the alternative half-precision format (AHP): no infinities or NaNs,
  saturation with IOC instead of overflow.

**Instructions** (`host/crates/backstep-emu/src/cpu/vfp.rs`): every
FPv4-SP instruction: VADD, VSUB, VMUL, VNMUL, VDIV, VMLA, VMLS, VNMLA,
VNMLS, VFMA, VFMS, VFNMA, VFNMS, VABS, VNEG, VSQRT, VMOV (immediate,
register, core register, two core registers to two singles or a
doubleword, 32-bit scalar), VCMP/VCMPE (with a register or #0.0), VCVT
and VCVTR between float and 32-bit integers, VCVT to and from 16- and
32-bit fixed point, VCVTB/VCVTT, VMRS (to a register or the APSR flags)
and VMSR, VLDR/VSTR (single or double, offset or literal), VLDM/VSTM in
both directions with and without writeback, VPUSH/VPOP; inside IT blocks
too. Double-precision registers exist only as pairs of singles (D0-D15).

**Access control and context.** Every instruction in the coprocessor
space first checks the coprocessor: any coprocessor other than cp10/cp11,
or the FPU while CPACR denies access (cp10 = 0b00, or 0b01 when
unprivileged), is a UsageFault with CFSR.NOCP. Decoding comes after this
check, so an UNDEFINED floating-point encoding (double-precision
arithmetic, ARMv8 forms such as VSEL) with the FPU disabled is NOCP too.
Then `ExecuteFPCheck()` runs: with FPCCR.ASPEN set and CONTROL.FPCA
clear, a new floating-point context is created (CONTROL.FPCA = 1, FPSCR
from FPDSCR). The SCS gains CPACR, FPCCR, FPCAR, FPDSCR and MVFR0-2.

**QEMU is the reference, where the manual leaves room.** The
differential test compares against QEMU 8.2's `cortex-m4`, so where the
architecture is UNPREDICTABLE or ambiguous the emulator does what QEMU
does, and each such case is queued for the board in
[HARDWARE_VERIFICATION.md](../HARDWARE_VERIFICATION.md):

- CPACR: only the cp10 field is checked, and the reserved value 0b10
  denies access (UNPREDICTABLE in the architecture; never tested
  against QEMU, only in a unit test).
- A new floating-point context sets all of FPSCR from FPDSCR, so the
  NZCV and exception flags become zero (the pseudocode copies bits
  26:22).
- With AHP, a NaN converts to a zero with the NaN's sign.
- VMOV of a 32-bit scalar (`VMOV.32 Dd[x], Rt`) is executed; the GNU
  assembler refuses it for FPv4-SP.
- VCVT from fixed point rounds to nearest whatever FPSCR says; VCVT to
  fixed point rounds towards zero (here QEMU and the manual agree).

QEMU's decoder calls the two fused negated forms by each other's names
(its "VFNMA" is the manual's VFNMS); the results per encoding are the
same, and the emulator uses the manual's names.

**Not yet:** an exception taken with CONTROL.FPCA set (an extended,
26-word frame, lazy state preservation, `EXC_RETURN` with bit 4 clear)
stops replay with `Stop::Unsupported`, and so does an FP instruction
with FPCCR.LSPACT set. That is part 2 of M7. Until then a recording that
takes an interrupt after using the FPU does not replay; firmware that
clears FPCCR.ASPEN (and does not rely on the hardware saving FP
registers) does.

**An instruction that stops changes nothing.** Every FP instruction
checks access, decodes and reads its operands (all of a VLDM's words)
before it writes any register, so `Cpu::step` does not save the 33
floating-point registers before each instruction. A VSTM interrupted by
a bus fault may have stored some words, as an STM does, and as on the
chip.

## How it is tested

- `scripts/emu-difftest.py --fpu`: random programs in which most
  instructions are floating point, mixed with the integer instructions
  and exceptions of the existing generator. The operands are biased to
  edge values (zeros, infinities, quiet and signalling NaNs with
  payloads, denormals, the largest and smallest normals, values near the
  integer and half-precision limits, tiny and huge values); FPSCR is set
  often, with every rounding mode and FZ, DN and AHP, and the cumulative
  flags mostly cleared so that each new flag shows. QEMU runs with
  `-d cpu,fpu`; S0-S31 and FPSCR are compared before every instruction,
  with the integer registers, and memory at the end. The run requires
  all 75 floating-point instruction classes (the integer run requires the 287 integer ones) and
  every kind of event: NOCP, each of the six exception flags raised,
  each rounding mode, FZ, DN and AHP in use. CI compares at least
  1,000,000 floating-point instructions (520 programs; 1,118,503 of
  3,962,814 instructions with seed 1, in which each exception flag was
  raised between 1,741 (DZC) and 121,626 (IXC) times).
- `cargo test`: 300,000 random edge-biased operands per operation
  against the host's IEEE arithmetic in round-to-nearest (NaNs compared
  as NaN only), and fixed cases for flags, rounding modes, FZ, DN,
  conversions and half precision; unit tests for NOCP, UNDEFINED
  encodings, privilege, the floating-point context, alignment and a VLDM
  stopped by a bus fault.
- Mutation check during development: 14 deliberate bugs (tininess after
  rounding, ties not to even, the rounding direction ignoring the sign,
  signalling NaNs not preferred, underflow not raised, the `inf * 0 +
  NaN` rule dropped, the sign of an exact zero sum, the NaN payload lost
  in half precision, IDC not raised, VCVT fixed rounding with FPSCR, the
  FPSCR write mask, VNMLS operand order, VCVTT not keeping the other
  half). The differential test found 13 within 60 programs. The
  fourteenth, an inexact square root missing its sticky bit, almost
  never changes a result: the root has 38 bits beyond the 24 kept.
- `scripts/emu-coverage.sh` counts `fpu.rs` and `vfp.rs` in the 90%
  line-coverage floor.

## Consequences

- The emulator runs firmware built for the FPU, as long as it takes no
  exception with a floating-point context active; replaying recordings
  of such firmware needs the recorder changes of part 2.
- The recording does not yet contain S0-S31, FPSCR, FPCCR or FPDSCR
  (CPACR is in the snapshot since M3): the recorder snapshot and the
  replay start state gain them in part 2, with the extended frames.
  Until then replay refuses a recording whose snapshot has the FPU
  enabled, with a message saying so, rather than start from a guessed
  floating-point state.
- The emulator grows by about 1,550 lines (a third of them the
  softfloat's own tests), and the trace tool prints the FP registers
  with `--fpu` in QEMU's format.
