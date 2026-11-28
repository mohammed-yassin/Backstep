//! Floating-point instructions of the Cortex-M4 FPU (FPv4-SP; ARMv7-M
//! Architecture Reference Manual A6, with the instruction pages of A7),
//! and the coprocessor space they live in.
//!
//! Decoding follows QEMU's (`target/arm/tcg/vfp.decode` and
//! `m-nocp.decode`), which the differential test compares against:
//!
//! * Every instruction in the coprocessor space (`111x 110x` and
//!   `111x 1110` in the first halfword) first checks the coprocessor:
//!   anything but cp10/cp11, or cp10/cp11 while CPACR denies access, is a
//!   `UsageFault` NOCP ([`Stop::NoCoprocessor`]), before decoding.
//! * Then the encoding is decoded; double-precision arithmetic (`sz = 1`)
//!   and the ARMv8 forms with the first halfword `1111 ...` are
//!   UNDEFINED on this single-precision unit. Double-precision registers
//!   still exist as pairs of single ones (D0-D15), so VLDR/VSTR/VLDM/VSTM
//!   and VMOV of a doubleword or a 32-bit scalar work with them.
//! * The instruction then runs the Arm pseudocode's `ExecuteFPCheck()`
//!   (QEMU's `vfp_access_check_m`): with FPCCR.ASPEN set and CONTROL.FPCA clear,
//!   a new floating-point context is created (FPSCR = FPDSCR, FPCA = 1).
//!
//! An instruction that stops (fault, UNDEFINED) changes nothing but what
//! `ExecuteFPCheck()` did (lazy preservation, a new context: QEMU keeps
//! those when the instruction then faults, and so does `Cpu::step`):
//! every instruction here checks and reads everything first and only then
//! writes registers, so `Cpu::step` need not save the floating-point
//! registers. (A store multiple may have written some words before a bus
//! fault, as STM does, and as on the real core.)

use super::{Cpu, Exec, Stop};
use crate::fpu::{self, fpscr, Env, Rounding};
use crate::memory::Bus;

/// FPCCR bits (Floating-Point Context Control Register).
pub mod fpccr {
    /// Automatic state preservation: an FP instruction sets CONTROL.FPCA.
    pub const ASPEN: u32 = 1 << 31;
    /// Lazy state preservation.
    pub const LSPEN: u32 = 1 << 30;
    /// Lazy state preservation is pending (space reserved, not written).
    pub const LSPACT: u32 = 1 << 0;
    /// The background context was unprivileged.
    pub const USER: u32 = 1 << 1;
    /// The background context was thread mode.
    pub const THREAD: u32 = 1 << 3;
    /// `HardFault` could have preempted the background context.
    pub const HFRDY: u32 = 1 << 4;
    /// `MemManage` could have preempted it.
    pub const MMRDY: u32 = 1 << 5;
    /// `BusFault` could have preempted it.
    pub const BFRDY: u32 = 1 << 6;
    /// `DebugMonitor` could have preempted it.
    pub const MONRDY: u32 = 1 << 8;
    /// `UsageFault` could have preempted it (an ARMv8-M bit, but QEMU sets
    /// it on ARMv7-M too; software cannot write it).
    pub const UFRDY: u32 = 1 << 10;
    /// Reset value: ASPEN and LSPEN set.
    pub const RESET: u32 = ASPEN | LSPEN;
    /// The bits software can write (ASPEN, LSPEN, MONRDY, BFRDY, MMRDY,
    /// HFRDY, THREAD, USER, LSPACT).
    pub const WRITABLE: u32 = 0xC000_017B;
    /// Every bit FPCCR can hold on this core (software-writable ones and
    /// UFRDY, which only exception entry sets).
    pub const WRITABLE_ALL: u32 = WRITABLE | UFRDY;
}

/// CPACR: the cp10 and cp11 fields (the only ones implemented).
pub const CPACR_WRITABLE: u32 = 0x00F0_0000;
/// FPDSCR: the default AHP, DN, FZ and `RMode`.
pub const FPDSCR_WRITABLE: u32 = 0x07C0_0000;
/// MVFR0 of the Cortex-M4 (single precision, divide, square root, short
/// vectors not supported, round modes).
pub const MVFR0: u32 = 0x1011_0021;
/// MVFR1 of the Cortex-M4 (FMA, half-precision conversion, FZ/DN).
pub const MVFR1: u32 = 0x1100_0011;

/// Counters of floating-point behaviour, for test coverage: how many
/// arithmetic instructions raised each exception flag, ran in each
/// rounding mode, with flush-to-zero or default NaN, and how many
/// half-precision conversions used the alternative format.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FpStats {
    /// By flag: IOC, DZC, OFC, UFC, IXC, IDC.
    pub flags: [u64; 6],
    /// By rounding mode: RN, RP, RM, RZ.
    pub rounding: [u64; 4],
    /// With FPSCR.FZ.
    pub flush_to_zero: u64,
    /// With FPSCR.DN.
    pub default_nan: u64,
    /// Half-precision conversions with FPSCR.AHP.
    pub alternative_half: u64,
}

/// Names of [`FpStats::flags`].
pub const FP_FLAG_NAMES: [&str; 6] = ["ioc", "dzc", "ofc", "ufc", "ixc", "idc"];

/// Single-precision register number from a 4-bit field and its extra bit
/// (`Vd:D`, `Vn:N`, `Vm:M`).
fn sreg(field: u32, bit: u32) -> usize {
    ((field << 1) | bit) as usize
}

/// Double-precision register number (`D:Vd`), if one of D0-D15.
fn dreg(field: u32, bit: u32) -> Option<usize> {
    if bit == 0 {
        Some(field as usize)
    } else {
        None
    }
}

impl Cpu {
    /// CPACR grants access to cp10: full access, or privileged-only
    /// access when privileged. As in QEMU, only the cp10 field is looked
    /// at, and the reserved value 0b10 denies access (the architecture
    /// makes both a cp11 field different from cp10 and 0b10
    /// UNPREDICTABLE).
    pub(super) fn fp_enabled(&self) -> bool {
        self.cpacr_allows(self.privileged())
    }

    pub(super) fn cpacr_allows(&self, privileged: bool) -> bool {
        match (self.scs.cpacr >> 20) & 3 {
            3 => true,
            1 => privileged,
            _ => false,
        }
    }

    /// `ExecuteFPCheck()` after the access check: lazy state preservation
    /// (`PreserveFPState`) if pending, then a new floating-point context
    /// if needed. Its effects stay even if the instruction then faults
    /// (`Cpu::step` keeps them, as QEMU does).
    fn fp_context(&mut self, bus: &mut dyn Bus, pc: u32) -> Exec {
        if self.scs.fpccr & fpccr::LSPACT != 0 {
            // The interrupted context's S0-S15 and FPSCR go into the
            // space its exception entry reserved. That context must have
            // had access to the FPU.
            if !self.cpacr_allows(self.scs.fpccr & fpccr::USER == 0) {
                return Err(Stop::Unsupported {
                    pc,
                    what: "CPACR forbids lazy floating-point state preservation (NOCP)",
                });
            }
            let at = self.scs.fpcar;
            let words: Vec<u32> = self.s[..16].iter().copied().chain([self.fpscr]).collect();
            for (i, v) in words.iter().enumerate() {
                if self
                    .write(bus, pc, at.wrapping_add(4 * i as u32), 4, *v)
                    .is_err()
                {
                    return Err(Stop::Unsupported {
                        pc,
                        what: "fault during lazy floating-point state preservation",
                    });
                }
            }
            self.scs.fpccr &= !fpccr::LSPACT;
            self.stats.fp_lazy_saves += 1;
        }
        if self.scs.fpccr & fpccr::ASPEN != 0 && self.control & 4 == 0 {
            // QEMU sets the whole FPSCR from FPDSCR (flags cleared).
            self.fpscr = self.scs.fpdscr & FPDSCR_WRITABLE;
            self.control |= 4;
        }
        self.fp_checked = true;
        Ok(())
    }

    /// What any floating-point instruction does before its own work: the
    /// access check, lazy state preservation and a new context
    /// (`ExecuteFPCheck()`). For replay, which must reproduce the effect
    /// of the recorder's own FP instructions without running them.
    ///
    /// # Errors
    /// [`Stop::NoCoprocessor`] if CPACR denies access; the others of
    /// lazy preservation (see [`Cpu::step`]).
    pub fn execute_fp_check(&mut self, bus: &mut dyn Bus) -> Exec {
        let pc = self.r[15];
        if !self.fp_enabled() {
            return Err(Stop::NoCoprocessor { pc });
        }
        self.fp_context(bus, pc)
    }

    /// The rounding and flags of an operation under the current FPSCR.
    fn env(&self) -> Env {
        Env::from_fpscr(self.fpscr)
    }

    /// Accumulate the exception flags an operation raised.
    fn raise_flags(&mut self, env: Env) {
        self.fpscr |= env.flags;
        let st = &mut self.fp_stats;
        for (i, bit) in [0, 1, 2, 3, 4, 7].iter().enumerate() {
            st.flags[i] += u64::from(env.flags >> bit & 1);
        }
        st.rounding[(self.fpscr >> fpscr::RMODE_SHIFT & 3) as usize] += 1;
        st.flush_to_zero += u64::from(env.fz);
        st.default_nan += u64::from(env.dn);
    }

    /// The coprocessor space: hw1 `111x 11xx xxxx xxxx`.
    pub(super) fn coprocessor(&mut self, bus: &mut dyn Bus, pc: u32, insn: u32) -> Exec {
        let undef = Err(Stop::Undefined { pc, insn });
        let hw1 = insn >> 16;
        if hw1 & 0x0F00 == 0x0F00 {
            return undef; // Advanced SIMD (not in ARMv7-M)
        }
        if (insn >> 9) & 7 != 5 || !self.fp_enabled() {
            return Err(Stop::NoCoprocessor { pc });
        }
        if hw1 & 0x1000 != 0 {
            return undef; // the ARMv8 unconditional forms (VSEL, VRINTA, ...)
        }
        let op1 = (hw1 >> 4) & 0x3F;
        if op1 & 0x20 == 0 {
            match op1 {
                0x04 | 0x05 => self.vmov_64(bus, pc, insn),
                0x00..=0x03 => undef,
                _ => self.load_store(bus, pc, insn),
            }
        } else if insn & 0x10 == 0 {
            self.fp_data_processing(bus, pc, insn)
        } else {
            self.transfer_32(bus, pc, insn)
        }
    }

    /// A6.6 32-bit transfers: VMOV (core register to or from a single
    /// register or a 32-bit scalar), VMRS, VMSR.
    fn transfer_32(&mut self, bus: &mut dyn Bus, pc: u32, insn: u32) -> Exec {
        let undef = Err(Stop::Undefined { pc, insn });
        let hw1 = insn >> 16;
        let to_core = hw1 & 0x10 != 0;
        let a = (hw1 >> 5) & 7;
        let rt = ((insn >> 12) & 0xF) as usize;
        let vn = hw1 & 0xF;
        let n_bit = (insn >> 7) & 1;
        let c = insn & 0x100 != 0;
        if insn & 0x6F != 0 {
            return undef; // bits 6:5 and 3:0 must be zero
        }
        match (c, a) {
            (false, 0) => {
                // VMOV between a core register and a single register.
                if rt >= 13 {
                    return undef;
                }
                self.fp_context(bus, pc)?;
                let s = sreg(vn, n_bit);
                if to_core {
                    self.r[rt] = self.s[s];
                } else {
                    self.s[s] = self.r[rt];
                }
                Ok(())
            }
            (false, 7) => {
                // VMSR/VMRS: only FPSCR exists on ARMv7-M.
                if vn != 1 || n_bit != 0 {
                    return undef;
                }
                if to_core {
                    if rt == 13 {
                        return undef;
                    }
                    self.fp_context(bus, pc)?;
                    if rt == 15 {
                        // VMRS APSR_nzcv, FPSCR (Q is kept).
                        let v = self.fpscr;
                        self.flags.n = v & (1 << 31) != 0;
                        self.flags.z = v & (1 << 30) != 0;
                        self.flags.c = v & (1 << 29) != 0;
                        self.flags.v = v & (1 << 28) != 0;
                    } else {
                        self.r[rt] = self.fpscr;
                    }
                } else {
                    if rt >= 13 {
                        return undef;
                    }
                    self.fp_context(bus, pc)?;
                    self.fpscr = self.r[rt] & fpscr::WRITABLE;
                }
                Ok(())
            }
            (true, 0 | 1) => {
                // VMOV.32 between a core register and Dn[x].
                let Some(d) = dreg(vn, n_bit) else {
                    return undef;
                };
                if rt >= 13 {
                    return undef;
                }
                self.fp_context(bus, pc)?;
                let s = 2 * d + (a & 1) as usize;
                if to_core {
                    self.r[rt] = self.s[s];
                } else {
                    self.s[s] = self.r[rt];
                }
                Ok(())
            }
            _ => undef, // VDUP and 8/16-bit scalars: Advanced SIMD
        }
    }

    /// A6.7 64-bit transfers: VMOV between two core registers and two
    /// single registers or a doubleword register.
    fn vmov_64(&mut self, bus: &mut dyn Bus, pc: u32, insn: u32) -> Exec {
        let undef = Err(Stop::Undefined { pc, insn });
        let hw1 = insn >> 16;
        let to_core = hw1 & 0x10 != 0;
        let rt2 = (hw1 & 0xF) as usize;
        let rt = ((insn >> 12) & 0xF) as usize;
        let vm = insn & 0xF;
        let m_bit = (insn >> 5) & 1;
        if insn & 0xD0 != 0x10 || rt >= 13 || rt2 >= 13 || (to_core && rt == rt2) {
            return undef; // bits 7:6 zero, bit 4 one; SP/PC UNPREDICTABLE
        }
        let first = if insn & 0x100 == 0 {
            let m = sreg(vm, m_bit);
            if m == 31 {
                return undef;
            }
            m
        } else {
            let Some(d) = dreg(vm, m_bit) else {
                return undef;
            };
            2 * d
        };
        self.fp_context(bus, pc)?;
        if to_core {
            self.r[rt] = self.s[first];
            self.r[rt2] = self.s[first + 1];
        } else {
            self.s[first] = self.r[rt];
            self.s[first + 1] = self.r[rt2];
        }
        Ok(())
    }

    /// A6.5 Extension register load/store: VLDR, VSTR, VLDM, VSTM, VPUSH,
    /// VPOP (single or doubleword registers; the transfers are words).
    fn load_store(&mut self, bus: &mut dyn Bus, pc: u32, insn: u32) -> Exec {
        let undef = Err(Stop::Undefined { pc, insn });
        let hw1 = insn >> 16;
        let p = hw1 & 0x100 != 0;
        let u = hw1 & 0x80 != 0;
        let w = hw1 & 0x20 != 0;
        let load = hw1 & 0x10 != 0;
        let n = (hw1 & 0xF) as usize;
        let d_bit = (hw1 >> 6) & 1;
        let vd = (insn >> 12) & 0xF;
        let double = insn & 0x100 != 0;
        let imm8 = insn & 0xFF;
        let base = if n == 15 {
            (pc.wrapping_add(4)) & !3
        } else {
            self.r[n]
        };
        // The first register and the number of words.
        let first = if double {
            let Some(d) = dreg(vd, d_bit) else {
                return undef;
            };
            2 * d
        } else {
            sreg(vd, d_bit)
        };
        if p && !w {
            // VLDR/VSTR: one register, offset +-imm8*4.
            let address = if u {
                base.wrapping_add(imm8 << 2)
            } else {
                base.wrapping_sub(imm8 << 2)
            };
            let words = if double { 2 } else { 1 };
            self.fp_context(bus, pc)?;
            return self.transfer_words(bus, pc, address, first, words, load);
        }
        // VLDM/VSTM: increment after (P=0, U=1) or decrement before with
        // writeback (P=1, U=0, W=1); anything else is not allocated.
        if p == u {
            return undef;
        }
        let words = if double {
            let regs = imm8 / 2;
            if regs == 0 || regs > 16 || first / 2 + regs as usize > 16 {
                return undef;
            }
            2 * regs as usize
        } else {
            if imm8 == 0 || first + imm8 as usize > 32 {
                return undef;
            }
            imm8 as usize
        };
        if n == 15 && w {
            return undef;
        }
        self.fp_context(bus, pc)?;
        let start = if p {
            base.wrapping_sub(imm8 << 2)
        } else {
            base
        };
        self.transfer_words(bus, pc, start, first, words, load)?;
        if w {
            // FLDMX/FSTMX (double, odd imm8) also write back imm8 words.
            self.r[n] = if u {
                base.wrapping_add(imm8 << 2)
            } else {
                base.wrapping_sub(imm8 << 2)
            };
        }
        Ok(())
    }

    /// Load or store `count` words from `s[first..]` at `address`, which
    /// must be word aligned (`MemA`). Loads write no register unless all
    /// reads succeed.
    fn transfer_words(
        &mut self,
        bus: &mut dyn Bus,
        pc: u32,
        address: u32,
        first: usize,
        count: usize,
        load: bool,
    ) -> Exec {
        Self::aligned(pc, address, 4)?;
        if load {
            let mut values = [0u32; 32];
            for (i, v) in values.iter_mut().enumerate().take(count) {
                *v = self.read(bus, pc, address.wrapping_add(4 * i as u32), 4)?;
            }
            self.s[first..first + count].copy_from_slice(&values[..count]);
        } else {
            for i in 0..count {
                let v = self.s[first + i];
                self.write(bus, pc, address.wrapping_add(4 * i as u32), 4, v)?;
            }
        }
        Ok(())
    }

    /// A6.4 Floating-point data-processing instructions.
    #[allow(clippy::too_many_lines)]
    fn fp_data_processing(&mut self, bus: &mut dyn Bus, pc: u32, insn: u32) -> Exec {
        let undef = Err(Stop::Undefined { pc, insn });
        if insn & 0x100 != 0 {
            return undef; // double precision: not on FPv4-SP
        }
        let hw1 = insn >> 16;
        let opc1 = (hw1 >> 4) & 0xB; // bits 23, 21, 20 (22 is D)
        let opc2 = hw1 & 0xF;
        let op = insn & 0x40 != 0; // bit 6
        let d = sreg((insn >> 12) & 0xF, (hw1 >> 6) & 1);
        let n = sreg(opc2, (insn >> 7) & 1);
        let m = sreg(insn & 0xF, (insn >> 5) & 1);
        match opc1 {
            0x0..=0x3 | 0x9 | 0xA => {}
            0x8 if !op => {}
            0xB => return self.fp_other_data_processing(bus, pc, insn, d, m),
            _ => return undef,
        }
        // The context switch may set FPSCR, so the controls are read after.
        self.fp_context(bus, pc)?;
        let (sd, sn, sm) = (self.s[d], self.s[n], self.s[m]);
        let neg = |x: u32| x ^ 0x8000_0000;
        let mut env = self.env();
        let result = match opc1 {
            0x0 => {
                // VMLA, VMLS: d + (+-)(n * m), rounded twice.
                let prod = fpu::mul(sn, sm, &mut env);
                fpu::add(sd, if op { neg(prod) } else { prod }, &mut env)
            }
            0x1 => {
                // VNMLS (op 0): -d + n*m; VNMLA (op 1): -d - n*m.
                let prod = fpu::mul(sn, sm, &mut env);
                fpu::add(neg(sd), if op { neg(prod) } else { prod }, &mut env)
            }
            0x2 => {
                // VMUL, VNMUL.
                let prod = fpu::mul(sn, sm, &mut env);
                if op {
                    neg(prod)
                } else {
                    prod
                }
            }
            0x3 => {
                if op {
                    fpu::sub(sn, sm, &mut env)
                } else {
                    fpu::add(sn, sm, &mut env)
                }
            }
            0x8 => fpu::div(sn, sm, &mut env),
            0x9 => {
                // VFNMS (op 0): -d + n*m; VFNMA (op 1): -d - n*m (the
                // Arm ARM's and gas's names; QEMU's decoder swaps the
                // names but computes the same). The negations come
                // first, so a negated NaN operand keeps its flipped sign.
                fpu::mul_add(neg(sd), if op { neg(sn) } else { sn }, sm, &mut env)
            }
            _ => {
                // VFMA (op 0): d + n*m; VFMS (op 1): d - n*m.
                fpu::mul_add(sd, if op { neg(sn) } else { sn }, sm, &mut env)
            }
        };
        self.s[d] = result;
        self.raise_flags(env);
        Ok(())
    }

    /// The "other" data-processing instructions (opc1 = 1x11): VMOV
    /// (immediate, register), VABS, VNEG, VSQRT, VCVTB/VCVTT, VCMP(E),
    /// VCVT between floating point and integer or fixed point.
    fn fp_other_data_processing(
        &mut self,
        bus: &mut dyn Bus,
        pc: u32,
        insn: u32,
        d: usize,
        m: usize,
    ) -> Exec {
        let undef = Err(Stop::Undefined { pc, insn });
        let hw1 = insn >> 16;
        let opc2 = hw1 & 0xF;
        let opc3 = (insn >> 6) & 3;
        if opc3 & 1 == 0 {
            // VMOV (immediate): bits 7 and 5:4 must be zero.
            if insn & 0xB0 != 0 {
                return undef;
            }
            self.fp_context(bus, pc)?;
            self.s[d] = fpu::expand_imm((opc2 << 4) | (insn & 0xF));
            return Ok(());
        }
        let high = opc3 & 2 != 0; // bit 7
                                  // Validity first: nothing may change before it is known the
                                  // instruction runs.
        match opc2 {
            0b0000..=0b0100 | 0b1000 | 0b1100 | 0b1101 => {}
            0b0101 if insn & 0x2F == 0 => {} // VCMP with #0.0: Vm, M zero
            0b1010 | 0b1011 | 0b1110 | 0b1111 => {
                // Fixed point: imm4:i may not exceed the size.
                let imm = ((insn & 0xF) << 1) | ((insn >> 5) & 1);
                if imm > if high { 32 } else { 16 } {
                    return undef;
                }
            }
            _ => return undef,
        }
        self.fp_context(bus, pc)?;
        let mut env = self.env();
        let sm = self.s[m];
        let sd = self.s[d];
        let value = match opc2 {
            0b0000 => {
                if high {
                    sm & 0x7FFF_FFFF // VABS
                } else {
                    sm // VMOV (register)
                }
            }
            0b0001 => {
                if high {
                    fpu::sqrt(sm, &mut env)
                } else {
                    sm ^ 0x8000_0000 // VNEG
                }
            }
            0b0010 => {
                // VCVTB/VCVTT.F32.F16: half in the bottom or top of Sm.
                let half = if high { sm >> 16 } else { sm & 0xFFFF };
                self.fp_stats.alternative_half += u64::from(env.ahp);
                fpu::half_to_single(half as u16, &mut env)
            }
            0b0011 => {
                // VCVTB/VCVTT.F16.F32: into the bottom or top of Sd.
                self.fp_stats.alternative_half += u64::from(env.ahp);
                let half = u32::from(fpu::single_to_half(sm, &mut env));
                if high {
                    (sd & 0xFFFF) | (half << 16)
                } else {
                    (sd & 0xFFFF_0000) | half
                }
            }
            0b0100 | 0b0101 => {
                // VCMP (bit 7 clear) / VCMPE (set): the flags go to FPSCR.
                let other = if opc2 == 0b0101 { 0 } else { sm };
                let nzcv = fpu::compare(sd, other, high, &mut env);
                self.fpscr = (self.fpscr & 0x0FFF_FFFF) | nzcv;
                self.raise_flags(env);
                return Ok(());
            }
            0b1000 => {
                // VCVT.F32.S32 (bit 7 set) / .U32, rounded as FPSCR says.
                let rounding = env.rounding;
                fpu::from_fixed(sm, 32, 0, !high, rounding, &mut env)
            }
            0b1100 | 0b1101 => {
                // VCVT(R).U32/S32.F32: bit 7 set rounds towards zero.
                let rounding = if high { Rounding::Zero } else { env.rounding };
                fpu::to_fixed(sm, 32, 0, opc2 == 0b1100, rounding, &mut env)
            }
            _ => {
                // VCVT between floating point and fixed point, in place:
                // opc2 = 1 op 1 U, bit 7 = sx (32-bit), imm4:i = size -
                // fraction bits.
                let to_fixed = opc2 & 4 != 0;
                let unsigned = opc2 & 1 != 0;
                let size = if high { 32 } else { 16 };
                let imm = ((insn & 0xF) << 1) | ((insn >> 5) & 1);
                let frac_bits = size - imm;
                if to_fixed {
                    fpu::to_fixed(sd, size, frac_bits, unsigned, Rounding::Zero, &mut env)
                } else {
                    // Rounds to nearest whatever FPSCR says (the
                    // architecture and QEMU agree).
                    fpu::from_fixed(sd, size, frac_bits, unsigned, Rounding::Nearest, &mut env)
                }
            }
        };
        self.s[d] = value;
        self.raise_flags(env);
        Ok(())
    }
}
