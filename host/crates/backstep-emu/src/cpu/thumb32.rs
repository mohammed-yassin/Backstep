//! 32-bit Thumb-2 instructions (ARMv7-M Architecture Reference Manual,
//! DDI 0403E, section A5.3, with the instruction pages of chapter A7).
//!
//! Each function implements one of the manual's decode tables; the table
//! is named in the function's comment. Encodings the manual marks
//! UNPREDICTABLE stop with [`Stop::Undefined`] instead of guessing. The
//! coprocessor space (floating point) is in `vfp.rs`.

use super::{Cpu, Exec, Stop};
use crate::alu::{add_with_carry, decode_imm_shift, shift_c, ShiftType};
use crate::memory::Bus;

/// 32-bit instructions that are UNPREDICTABLE in an IT block (B<cond>.W),
/// or anywhere in one but last (branches and loads into PC); A7.3.3.
pub(super) fn not_allowed_in_it(insn: u32, last: bool) -> bool {
    let hw1 = insn >> 16;
    let hw2 = insn & 0xFFFF;
    let branch_space = hw1 & 0xF800 == 0xF000 && hw2 & 0x8000 != 0;
    // B T3: op1 = 0x0 and op (hw1[10:4]) not x111xxx.
    let cond_branch = branch_space && hw2 & 0x5000 == 0 && (hw1 >> 7) & 7 != 7;
    let branch = (branch_space && hw2 & 0x1000 != 0)                 // B.W T4, BL
        || (hw1 & 0xFFF0 == 0xE8D0 && hw2 & 0xFFE0 == 0xF000)          // TBB/TBH
        || (hw1 & 0xFF70 == 0xF850 && hw2 >> 12 == 15)                 // LDR PC, ...
        || (hw1 & 0xFE50 == 0xE810 && hw2 & 0x8000 != 0); // LDM/POP {.., pc}
    cond_branch || (branch && !last)
}

/// `ThumbExpandImm_C(imm12, carry_in)` (A5.3.2); `None` if UNPREDICTABLE.
fn thumb_expand_imm_c(imm12: u32, carry: bool) -> Option<(u32, bool)> {
    if imm12 >> 10 == 0 {
        let imm8 = imm12 & 0xFF;
        let kind = (imm12 >> 8) & 3;
        if kind != 0 && imm8 == 0 {
            return None;
        }
        let v = imm8 * [1, 0x0001_0001, 0x0100_0100, 0x0101_0101][kind as usize];
        Some((v, carry))
    } else {
        let v = (0x80 | (imm12 & 0x7F)).rotate_right(imm12 >> 7);
        Some((v, v >> 31 != 0))
    }
}

/// `SignedSatQ(i, n)`: (result, saturated).
fn signed_sat(i: i64, n: u32) -> (i64, bool) {
    let max = (1i64 << (n - 1)) - 1;
    let min = -(1i64 << (n - 1));
    if i > max {
        (max, true)
    } else if i < min {
        (min, true)
    } else {
        (i, false)
    }
}

/// `UnsignedSatQ(i, n)`: (result, saturated).
fn unsigned_sat(i: i64, n: u32) -> (i64, bool) {
    let max = (1i64 << n) - 1;
    if i > max {
        (max, true)
    } else if i < 0 {
        (0, true)
    } else {
        (i, false)
    }
}

/// Signed halfword `half` (0 = bits 15:0, 1 = bits 31:16) of `v`.
fn sh(v: u32, half: u32) -> i64 {
    i64::from((v >> (16 * half)) as u16 as i16)
}

fn bad(r: usize) -> bool {
    r == 13 || r == 15
}

impl Cpu {
    /// Execute a 32-bit instruction whose condition passed (A5.3).
    pub(super) fn exec32(&mut self, bus: &mut dyn Bus, pc: u32, insn: u32) -> Exec {
        let hw1 = insn >> 16;
        let hw2 = insn & 0xFFFF;
        let op2 = (hw1 >> 4) & 0x7F;
        match (hw1 >> 11) & 3 {
            1 => {
                if op2 & 0x64 == 0 {
                    self.load_store_multiple(bus, pc, insn)
                } else if op2 & 0x64 == 0x04 {
                    self.load_store_dual(bus, pc, insn)
                } else if op2 & 0x60 == 0x20 {
                    self.dp_shifted_register(pc, insn)
                } else {
                    self.coprocessor(bus, pc, insn)
                }
            }
            2 => {
                if hw2 & 0x8000 != 0 {
                    self.branch_misc(pc, insn)
                } else if op2 & 0x20 == 0 {
                    self.dp_modified_immediate(pc, insn)
                } else {
                    self.dp_plain_immediate(pc, insn)
                }
            }
            _ => {
                if op2 & 0x71 == 0x00 || matches!(op2 & 0x67, 0x01 | 0x03 | 0x05) {
                    self.load_store_single(bus, pc, insn)
                } else if op2 & 0x70 == 0x20 {
                    self.dp_register(pc, insn)
                } else if op2 & 0x78 == 0x30 {
                    self.multiply(pc, insn)
                } else if op2 & 0x78 == 0x38 {
                    self.long_multiply_divide(pc, insn)
                } else if op2 & 0x40 != 0 {
                    self.coprocessor(bus, pc, insn)
                } else {
                    Err(Stop::Undefined { pc, insn })
                }
            }
        }
    }

    /// A5.3.5 Load/store multiple: STM(IA), LDM(IA), POP.W, STMDB, PUSH.W,
    /// LDMDB (A7.7.40/41/42/98/99/157/158).
    fn load_store_multiple(&mut self, bus: &mut dyn Bus, pc: u32, insn: u32) -> Exec {
        let undef = Err(Stop::Undefined { pc, insn });
        let hw1 = insn >> 16;
        let hw2 = insn & 0xFFFF;
        let op = (hw1 >> 7) & 3;
        let wback = hw1 & 0x20 != 0;
        let load = hw1 & 0x10 != 0;
        let n = (hw1 & 0xF) as usize;
        if op == 0 || op == 3 || hw2 & 0x2000 != 0 || (!load && hw2 & 0x8000 != 0) {
            return undef; // SRS/RFE (not in ARMv7-M), SP/PC not allowed
        }
        let list = hw2;
        if n == 15 || list.count_ones() < 2 || (load && list & 0xC000 == 0xC000) {
            return undef;
        }
        if wback && list & (1 << n) != 0 {
            return undef;
        }
        let count = list.count_ones();
        let base = self.r[n];
        let start = if op == 1 {
            base
        } else {
            base.wrapping_sub(4 * count)
        };
        Self::aligned(pc, start, 4)?;
        let end = if op == 1 {
            base.wrapping_add(4 * count)
        } else {
            start
        };
        let mut addr = start;
        if load {
            let mut values = [0u32; 16];
            for (i, slot) in values.iter_mut().enumerate() {
                if list & (1 << i) != 0 {
                    *slot = self.read(bus, pc, addr, 4)?;
                    addr = addr.wrapping_add(4);
                }
            }
            for (i, v) in values.iter().enumerate().take(15) {
                if list & (1 << i) != 0 {
                    self.r[i] = *v;
                }
            }
            if wback {
                self.r[n] = end;
            }
            if list & 0x8000 != 0 {
                return self.bx_write_pc(pc, values[15]);
            }
        } else {
            for i in 0..15 {
                if list & (1 << i) != 0 {
                    let v = self.r[i];
                    self.write(bus, pc, addr, 4, v)?;
                    addr = addr.wrapping_add(4);
                }
            }
            if wback {
                self.r[n] = end;
            }
        }
        Ok(())
    }

    /// A5.3.6 Load/store dual or exclusive, table branch: LDRD, STRD,
    /// LDREX(B/H), STREX(B/H), TBB, TBH.
    fn load_store_dual(&mut self, bus: &mut dyn Bus, pc: u32, insn: u32) -> Exec {
        let undef = Err(Stop::Undefined { pc, insn });
        let hw1 = insn >> 16;
        let hw2 = insn & 0xFFFF;
        let p = hw1 & 0x100 != 0;
        let u = hw1 & 0x80 != 0;
        let w = hw1 & 0x20 != 0;
        let load = hw1 & 0x10 != 0;
        let n = (hw1 & 0xF) as usize;
        let t = (hw2 >> 12) as usize;
        let t2 = ((hw2 >> 8) & 0xF) as usize;
        let imm8 = hw2 & 0xFF;
        if !p && !w {
            if !u {
                // LDREX / STREX (A7.7.52 / A7.7.167): [Rn, #imm8 * 4].
                let address = self.r[n].wrapping_add(imm8 << 2);
                if load {
                    if t2 != 15 || bad(t) || n == 15 {
                        return undef;
                    }
                    Self::aligned(pc, address, 4)?;
                    self.r[t] = self.read(bus, pc, address, 4)?;
                    self.monitor = Some(address);
                    return Ok(());
                }
                let d = t2;
                if bad(d) || bad(t) || n == 15 || d == n || d == t {
                    return undef;
                }
                return self.store_exclusive(bus, pc, d, t, address, 4);
            }
            let op3 = (hw2 >> 4) & 0xF;
            let m = (hw2 & 0xF) as usize;
            if load {
                match op3 {
                    0 | 1 => {
                        // TBB / TBH (A7.7.182).
                        if hw2 & 0xFF00 != 0xF000 || n == 13 || bad(m) {
                            return undef;
                        }
                        let base = self.reg(n, pc);
                        let halfwords = if op3 == 1 {
                            let a = base.wrapping_add(self.r[m] << 1);
                            self.read(bus, pc, a, 2)?
                        } else {
                            let a = base.wrapping_add(self.r[m]);
                            self.read(bus, pc, a, 1)?
                        };
                        self.r[15] = pc.wrapping_add(4).wrapping_add(2 * halfwords);
                        Ok(())
                    }
                    4 | 5 => {
                        // LDREXB / LDREXH (A7.7.53 / A7.7.54).
                        if hw2 & 0x0F0F != 0x0F0F || bad(t) || n == 15 {
                            return undef;
                        }
                        let size = if op3 == 4 { 1 } else { 2 };
                        let address = self.r[n];
                        Self::aligned(pc, address, u32::from(size))?;
                        self.r[t] = self.read(bus, pc, address, size)?;
                        self.monitor = Some(address);
                        Ok(())
                    }
                    _ => undef,
                }
            } else {
                // STREXB / STREXH (A7.7.168 / A7.7.169).
                if (op3 != 4 && op3 != 5) || t2 != 15 {
                    return undef;
                }
                let d = m;
                if bad(d) || bad(t) || n == 15 || d == n || d == t {
                    return undef;
                }
                let size = if op3 == 4 { 1 } else { 2 };
                let address = self.r[n];
                self.store_exclusive(bus, pc, d, t, address, size)
            }
        } else {
            // LDRD / STRD (immediate, literal) A7.7.49 / A7.7.50 / A7.7.163.
            let imm32 = imm8 << 2;
            if bad(t) || bad(t2) || (wback_conflict(w, n, t, t2)) {
                return undef;
            }
            let (address, offset_addr) = if n == 15 {
                if !load || w {
                    return undef;
                }
                let base = self.reg(15, pc) & !3;
                let a = if u {
                    base.wrapping_add(imm32)
                } else {
                    base.wrapping_sub(imm32)
                };
                (a, a)
            } else {
                let rn = self.r[n];
                let off = if u {
                    rn.wrapping_add(imm32)
                } else {
                    rn.wrapping_sub(imm32)
                };
                (if p { off } else { rn }, off)
            };
            Self::aligned(pc, address, 4)?;
            if load {
                if t == t2 {
                    return undef;
                }
                let lo = self.read(bus, pc, address, 4)?;
                let hi = self.read(bus, pc, address.wrapping_add(4), 4)?;
                self.r[t] = lo;
                self.r[t2] = hi;
            } else {
                let (lo, hi) = (self.r[t], self.r[t2]);
                self.write(bus, pc, address, 4, lo)?;
                self.write(bus, pc, address.wrapping_add(4), 4, hi)?;
            }
            if w {
                self.r[n] = offset_addr;
            }
            Ok(())
        }
    }

    /// The store half of an exclusive pair (`ExclusiveMonitorsPass`):
    /// writes only while the local monitor is in the Exclusive state, and,
    /// if `exclusive_tags_address`, holds this address. `Rd` = 0 on
    /// success, 1 on failure. Either way the monitor is cleared.
    fn store_exclusive(
        &mut self,
        bus: &mut dyn Bus,
        pc: u32,
        d: usize,
        t: usize,
        address: u32,
        size: u8,
    ) -> Exec {
        Self::aligned(pc, address, u32::from(size))?;
        let pass = match self.monitor {
            Some(marked) => marked == address || !self.exclusive_tags_address,
            None => false,
        };
        if pass {
            let v = self.r[t];
            self.write(bus, pc, address, size, v)?;
        }
        self.monitor = None;
        self.r[d] = u32::from(!pass);
        Ok(())
    }

    /// A5.3.7-A5.3.10 Load/store single data item: LDR, LDRB, LDRH,
    /// LDRSB, LDRSH, STR, STRB, STRH in every addressing form (12-bit
    /// immediate, 8-bit immediate with pre/post-indexing and writeback,
    /// register with LSL #0-3, literal), the unprivileged LDRT-family, and
    /// the memory hints PLD/PLI.
    fn load_store_single(&mut self, bus: &mut dyn Bus, pc: u32, insn: u32) -> Exec {
        let undef = Err(Stop::Undefined { pc, insn });
        let hw1 = insn >> 16;
        let hw2 = insn & 0xFFFF;
        let signed = hw1 & 0x100 != 0;
        let a = hw1 & 0x80 != 0;
        let size_field = (hw1 >> 5) & 3;
        let load = hw1 & 0x10 != 0;
        let n = (hw1 & 0xF) as usize;
        let t = (hw2 >> 12) as usize;
        if size_field == 3 || (!load && signed) || (signed && size_field == 2) {
            return undef;
        }
        let size = 1u8 << size_field;
        let (address, writeback) = if n == 15 {
            if !load {
                return undef;
            }
            let base = self.reg(15, pc) & !3;
            let imm12 = hw2 & 0xFFF;
            let address = if a {
                base.wrapping_add(imm12)
            } else {
                base.wrapping_sub(imm12)
            };
            (address, None)
        } else if a {
            (self.r[n].wrapping_add(hw2 & 0xFFF), None)
        } else if hw2 & 0x800 != 0 {
            let p = hw2 & 0x400 != 0;
            let u = hw2 & 0x200 != 0;
            let w = hw2 & 0x100 != 0;
            if !p && !w {
                return undef;
            }
            let rn = self.r[n];
            let imm8 = hw2 & 0xFF;
            let off = if u {
                rn.wrapping_add(imm8)
            } else {
                rn.wrapping_sub(imm8)
            };
            (if p { off } else { rn }, w.then_some(off))
        } else if hw2 & 0xFC0 == 0 {
            let m = (hw2 & 0xF) as usize;
            if bad(m) {
                return undef;
            }
            let address = self.r[n].wrapping_add(self.r[m] << ((hw2 >> 4) & 3));
            (address, None)
        } else {
            return undef;
        };
        if writeback.is_some() && n == t {
            return undef;
        }
        if load {
            if t == 15 && size != 4 {
                // PLD, PLI and unallocated memory hints execute as NOPs.
                return if writeback.is_some() { undef } else { Ok(()) };
            }
            if t == 13 && size != 4 {
                return undef;
            }
            if t == 15 && address & 3 != 0 {
                return undef;
            }
            let v = self.read(bus, pc, address, size)?;
            let v = match (signed, size) {
                (true, 1) => v as u8 as i8 as i32 as u32,
                (true, _) => v as u16 as i16 as i32 as u32,
                _ => v,
            };
            if let Some(wb) = writeback {
                self.r[n] = wb;
            }
            if t == 15 {
                return self.bx_write_pc(pc, v);
            }
            self.r[t] = v;
        } else {
            if t == 15 || (t == 13 && size != 4) {
                return undef;
            }
            let v = self.r[t];
            self.write(bus, pc, address, size, v)?;
            if let Some(wb) = writeback {
                self.r[n] = wb;
            }
        }
        Ok(())
    }

    /// A5.3.1 Data processing (modified immediate).
    fn dp_modified_immediate(&mut self, pc: u32, insn: u32) -> Exec {
        let hw1 = insn >> 16;
        let hw2 = insn & 0xFFFF;
        let imm12 = ((hw1 >> 10) & 1) << 11 | ((hw2 >> 12) & 7) << 8 | (hw2 & 0xFF);
        let Some((imm, carry)) = thumb_expand_imm_c(imm12, self.flags.c) else {
            return Err(Stop::Undefined { pc, insn });
        };
        let op = (hw1 >> 5) & 0xF;
        let s = hw1 & 0x10 != 0;
        let n = (hw1 & 0xF) as usize;
        let d = ((hw2 >> 8) & 0xF) as usize;
        self.data_processing(pc, insn, op, s, n, d, imm, carry)
    }

    /// A5.3.11 Data processing (shifted register), including the MOV.W and
    /// immediate-shift (LSL/LSR/ASR/ROR/RRX) forms and PKHBT/PKHTB.
    fn dp_shifted_register(&mut self, pc: u32, insn: u32) -> Exec {
        let undef = Err(Stop::Undefined { pc, insn });
        let hw1 = insn >> 16;
        let hw2 = insn & 0xFFFF;
        let op = (hw1 >> 5) & 0xF;
        let s = hw1 & 0x10 != 0;
        let n = (hw1 & 0xF) as usize;
        let d = ((hw2 >> 8) & 0xF) as usize;
        let m = (hw2 & 0xF) as usize;
        let imm5 = ((hw2 >> 12) & 7) << 2 | ((hw2 >> 6) & 3);
        let ty = (hw2 >> 4) & 3;
        if hw2 & 0x8000 != 0 {
            return undef;
        }
        if op == 6 {
            // PKHBT / PKHTB (A7.7.95).
            if s || ty & 1 != 0 || bad(d) || bad(n) || bad(m) {
                return undef;
            }
            let tb = ty & 2 != 0;
            let (kind, amount) = decode_imm_shift(ty, imm5);
            let (operand, _) = shift_c(self.r[m], kind, amount, self.flags.c);
            let rn = self.r[n];
            self.r[d] = if tb {
                (rn & 0xFFFF_0000) | (operand & 0xFFFF)
            } else {
                (operand & 0xFFFF_0000) | (rn & 0xFFFF)
            };
            return Ok(());
        }
        let (kind, amount) = decode_imm_shift(ty, imm5);
        let is_mov = op == 2 && n == 15;
        let plain_mov = is_mov && kind == ShiftType::Lsl && amount == 0;
        if m == 15 {
            return undef;
        }
        if m == 13 && (s || !plain_mov) {
            return undef;
        }
        if plain_mov && !s {
            // MOV.W (register) T3 without flags allows SP as one operand.
            if d == 15 || (d == 13 && m == 13) {
                return undef;
            }
            self.r[d] = self.r[m];
            return Ok(());
        }
        let (operand, carry) = shift_c(self.r[m], kind, amount, self.flags.c);
        if matches!(op, 8 | 13) && n == 13 && d == 13 && (kind != ShiftType::Lsl || amount > 3) {
            return undef; // ADD/SUB (SP plus/minus register) T3
        }
        self.data_processing(pc, insn, op, s, n, d, operand, carry)
    }

    /// The operation table shared by A5.3.1 and A5.3.11: AND/TST, BIC,
    /// ORR/MOV, ORN/MVN, EOR/TEQ, ADD/CMN, ADC, SBC, SUB/CMP, RSB.
    /// `carry` is the shifter carry used by the logical operations.
    #[allow(clippy::too_many_arguments, clippy::fn_params_excessive_bools)]
    fn data_processing(
        &mut self,
        pc: u32,
        insn: u32,
        op: u32,
        s: bool,
        n: usize,
        d: usize,
        operand: u32,
        carry: bool,
    ) -> Exec {
        let undef = Err(Stop::Undefined { pc, insn });
        let test = d == 15 && s && matches!(op, 0 | 4 | 8 | 13);
        let unary = matches!(op, 2 | 3) && n == 15;
        let sp_arith = matches!(op, 8 | 13) && n == 13 && !test;
        if test {
            if n == 15 || (n == 13 && !matches!(op, 8 | 13)) {
                return undef;
            }
        } else if sp_arith {
            if d == 15 {
                return undef;
            }
        } else if bad(d) || (!unary && bad(n)) {
            return undef;
        }
        let rn = self.r[n];
        let c_in = self.flags.c;
        // (result, carry, overflow); overflow None = logical operation.
        let (result, c, v) = match op {
            0 => (rn & operand, carry, None),
            1 => (rn & !operand, carry, None),
            2 => (if unary { operand } else { rn | operand }, carry, None),
            3 => (if unary { !operand } else { rn | !operand }, carry, None),
            4 => (rn ^ operand, carry, None),
            8 => {
                let (r, c, v) = add_with_carry(rn, operand, false);
                (r, c, Some(v))
            }
            10 => {
                let (r, c, v) = add_with_carry(rn, operand, c_in);
                (r, c, Some(v))
            }
            11 => {
                let (r, c, v) = add_with_carry(rn, !operand, c_in);
                (r, c, Some(v))
            }
            13 => {
                let (r, c, v) = add_with_carry(rn, !operand, true);
                (r, c, Some(v))
            }
            14 => {
                let (r, c, v) = add_with_carry(!rn, operand, true);
                (r, c, Some(v))
            }
            _ => return undef,
        };
        if !test {
            self.r[d] = result;
        }
        if s {
            self.set_nz(result);
            self.flags.c = c;
            if let Some(v) = v {
                self.flags.v = v;
            }
        }
        Ok(())
    }

    /// A5.3.3 Data processing (plain binary immediate): ADDW, SUBW, ADR,
    /// MOVW, MOVT, SSAT, SSAT16, USAT, USAT16, SBFX, UBFX, BFI, BFC.
    fn dp_plain_immediate(&mut self, pc: u32, insn: u32) -> Exec {
        let undef = Err(Stop::Undefined { pc, insn });
        let hw1 = insn >> 16;
        let hw2 = insn & 0xFFFF;
        let op = (hw1 >> 4) & 0x1F;
        let n = (hw1 & 0xF) as usize;
        let d = ((hw2 >> 8) & 0xF) as usize;
        let imm3 = (hw2 >> 12) & 7;
        let imm2 = (hw2 >> 6) & 3;
        let imm12 = ((hw1 >> 10) & 1) << 11 | imm3 << 8 | (hw2 & 0xFF);
        let low5 = hw2 & 0x1F;
        let lsb = imm3 << 2 | imm2;
        match op {
            0x00 | 0x0A => {
                // ADDW/SUBW (A7.7.3 T4, A7.7.174 T4), ADR (A7.7.7 T2/T3),
                // ADD/SUB SP plus immediate (T4/T3).
                let base = match n {
                    15 if !bad(d) => self.reg(15, pc) & !3,
                    13 if d != 15 => self.r[13],
                    0..=12 | 14 if !bad(d) => self.r[n],
                    _ => return undef,
                };
                self.r[d] = if op == 0 {
                    base.wrapping_add(imm12)
                } else {
                    base.wrapping_sub(imm12)
                };
            }
            0x04 | 0x0C => {
                // MOVW (A7.7.75 T3), MOVT (A7.7.78).
                if bad(d) {
                    return undef;
                }
                let imm16 = (n as u32) << 12 | imm12;
                self.r[d] = if op == 4 {
                    imm16
                } else {
                    (imm16 << 16) | (self.r[d] & 0xFFFF)
                };
            }
            0x10 | 0x12 | 0x18 | 0x1A => {
                // SSAT/SSAT16 (A7.7.130/131), USAT/USAT16 (A7.7.212/213).
                if bad(d) || bad(n) || hw2 & 0x20 != 0 {
                    return undef;
                }
                let unsigned = op >= 0x18;
                let asr = op & 2 != 0;
                let rn = self.r[n];
                let (result, sat) = if asr && lsb == 0 {
                    if hw2 & 0x10 != 0 {
                        return undef;
                    }
                    let width = if unsigned {
                        low5 & 0xF
                    } else {
                        (low5 & 0xF) + 1
                    };
                    let mut q = false;
                    let mut out = 0u32;
                    for half in 0..2 {
                        let (r, s) = if unsigned {
                            unsigned_sat(sh(rn, half), width)
                        } else {
                            signed_sat(sh(rn, half), width)
                        };
                        q |= s;
                        out |= ((r as u32) & 0xFFFF) << (16 * half);
                    }
                    (out, q)
                } else {
                    let (kind, amount) = decode_imm_shift(if asr { 2 } else { 0 }, lsb);
                    let (operand, _) = shift_c(rn, kind, amount, self.flags.c);
                    let v = i64::from(operand as i32);
                    let (r, s) = if unsigned {
                        unsigned_sat(v, low5)
                    } else {
                        signed_sat(v, low5 + 1)
                    };
                    (r as u32, s)
                };
                self.r[d] = result;
                if sat {
                    self.flags.q = true;
                }
            }
            0x14 | 0x1C => {
                // SBFX (A7.7.126), UBFX (A7.7.193).
                let msb = lsb + low5;
                if msb > 31 || bad(d) || bad(n) || hw2 & 0x20 != 0 {
                    return undef;
                }
                let field = self.r[n] << (31 - msb);
                self.r[d] = if op == 0x14 {
                    ((field as i32) >> (31 - low5)) as u32
                } else {
                    field >> (31 - low5)
                };
            }
            0x16 => {
                // BFI (A7.7.14), BFC (A7.7.13; Rn = 1111).
                let msb = low5;
                if msb < lsb || bad(d) || n == 13 || hw2 & 0x20 != 0 {
                    return undef;
                }
                let width = msb - lsb + 1;
                let mask = (u32::MAX >> (32 - width)) << lsb;
                let src = if n == 15 { 0 } else { self.r[n] << lsb };
                self.r[d] = (self.r[d] & !mask) | (src & mask);
            }
            _ => return undef,
        }
        Ok(())
    }

    /// A5.3.4 Branches and miscellaneous control: B.W (T3, T4), BL, MSR,
    /// MRS, NOP.W and the other hints, CLREX, DSB, DMB, ISB, UDF.W.
    fn branch_misc(&mut self, pc: u32, insn: u32) -> Exec {
        let undef = Err(Stop::Undefined { pc, insn });
        let hw1 = insn >> 16;
        let hw2 = insn & 0xFFFF;
        let op = (hw1 >> 4) & 0x7F;
        let op1 = (hw2 >> 12) & 7;
        let s = (hw1 >> 10) & 1;
        let j1 = (hw2 >> 13) & 1;
        let j2 = (hw2 >> 11) & 1;
        let imm11 = hw2 & 0x7FF;
        if op1 == 2 && op == 0x7F {
            return undef; // UDF.W: permanently undefined
        }
        if op1 & 5 == 0 {
            if op & 0x38 != 0x38 {
                // B<cond>.W T3 (A7.7.12).
                let cond = (hw1 >> 6) & 0xF;
                let imm = s << 20 | j2 << 19 | j1 << 18 | (hw1 & 0x3F) << 12 | imm11 << 1;
                let offset = ((imm << 11) as i32 >> 11) as u32;
                if self.flags.condition_passed(cond) {
                    self.r[15] = pc.wrapping_add(4).wrapping_add(offset);
                }
                return Ok(());
            }
            return match op {
                0x38 => self.msr(pc, insn),
                0x3E => self.mrs(pc, insn),
                0x3A => {
                    // Hints (NOP, YIELD, WFE, WFI, SEV, DBG): no
                    // architectural effect here; during replay interrupts
                    // come from the recording.
                    if hw2 & 0x0700 != 0 {
                        undef
                    } else {
                        Ok(())
                    }
                }
                0x3B => match (hw2 >> 4) & 0xF {
                    2 => {
                        self.monitor = None; // CLREX
                        Ok(())
                    }
                    4..=6 => Ok(()), // DSB, DMB, ISB
                    _ => undef,
                },
                _ => undef,
            };
        }
        if op1 & 1 == 0 {
            return undef; // BLX (immediate) does not exist in ARMv7-M
        }
        // B.W T4 (A7.7.12), BL (A7.7.18).
        let i1 = !(j1 ^ s) & 1;
        let i2 = !(j2 ^ s) & 1;
        let imm = s << 24 | i1 << 23 | i2 << 22 | (hw1 & 0x3FF) << 12 | imm11 << 1;
        let offset = ((imm << 7) as i32 >> 7) as u32;
        let next = pc.wrapping_add(4);
        if op1 & 4 != 0 {
            self.r[14] = next | 1;
        }
        self.r[15] = next.wrapping_add(offset);
        Ok(())
    }

    /// MSR (B5.2.3): APSR (flags, GE), MSP, PSP, PRIMASK, BASEPRI,
    /// `BASEPRI_MAX`, FAULTMASK, CONTROL.
    fn msr(&mut self, pc: u32, insn: u32) -> Exec {
        let undef = Err(Stop::Undefined { pc, insn });
        let hw1 = insn >> 16;
        let hw2 = insn & 0xFFFF;
        let n = (hw1 & 0xF) as usize;
        let mask = (hw2 >> 10) & 3;
        let sysm = hw2 & 0xFF;
        if bad(n) || hw2 & 0x2300 != 0 || mask == 0 || (mask != 2 && sysm > 3) {
            return undef;
        }
        let v = self.r[n];
        let privileged = self.privileged();
        match sysm {
            0..=3 => {
                if mask & 2 != 0 {
                    self.flags = crate::alu::Flags::from_bits(v);
                }
                if mask & 1 != 0 {
                    self.ge = ((v >> 16) & 0xF) as u8;
                }
            }
            5..=7 => {} // IPSR and EPSR are not writable.
            8 | 9 => {
                if privileged {
                    let main = sysm == 8;
                    if main == self.using_psp() {
                        self.sp_banked = v & !3;
                    } else {
                        self.r[13] = v & !3;
                    }
                }
            }
            16 => {
                if privileged {
                    self.primask = v & 1 != 0;
                }
            }
            17 | 18 => {
                if privileged {
                    let new = (v as u8) & self.priority_mask;
                    if sysm == 17 || (new != 0 && (new < self.basepri || self.basepri == 0)) {
                        self.basepri = new;
                    }
                }
            }
            19 => {
                if privileged && (v & 1 == 0 || self.may_set_faultmask()) {
                    self.faultmask = v & 1 != 0;
                }
            }
            20 => {
                if privileged {
                    let mut control = (self.control & !1) | (v & 1);
                    control = (control & !4) | (v & 4); // FPCA (FPU present)
                    if self.ipsr == 0 {
                        let spsel = v & 2;
                        if spsel != self.control & 2 {
                            std::mem::swap(&mut self.r[13], &mut self.sp_banked);
                        }
                        control = (control & !2) | spsel;
                    }
                    self.control = control;
                }
            }
            _ => return undef,
        }
        Ok(())
    }

    /// MRS (B5.2.2).
    fn mrs(&mut self, pc: u32, insn: u32) -> Exec {
        let undef = Err(Stop::Undefined { pc, insn });
        let hw1 = insn >> 16;
        let hw2 = insn & 0xFFFF;
        let d = ((hw2 >> 8) & 0xF) as usize;
        let sysm = hw2 & 0xFF;
        if bad(d) || hw1 & 0xF != 0xF || hw2 & 0x3000 != 0 {
            return undef;
        }
        let privileged = self.privileged();
        self.r[d] = match sysm {
            0..=3 | 5..=7 => {
                let mut v = 0;
                if sysm & 1 != 0 && privileged {
                    v |= self.ipsr;
                }
                if sysm & 4 == 0 {
                    v |= self.flags.to_bits() | u32::from(self.ge) << 16;
                }
                v // EPSR (T, IT) reads as zero
            }
            8 if privileged => self.msp(),
            9 if privileged => self.psp(),
            8 | 9 => 0,
            16 => u32::from(self.primask),
            17 | 18 => u32::from(self.basepri),
            19 => u32::from(self.faultmask),
            20 => self.control & 7,
            _ => return undef,
        };
        Ok(())
    }

    /// A5.3.12 Data processing (register): register-controlled shifts,
    /// extends with optional add, and the parallel and miscellaneous
    /// groups (A5.3.13-A5.3.15).
    fn dp_register(&mut self, pc: u32, insn: u32) -> Exec {
        let undef = Err(Stop::Undefined { pc, insn });
        let hw1 = insn >> 16;
        let hw2 = insn & 0xFFFF;
        let op1 = (hw1 >> 4) & 0xF;
        let op2 = (hw2 >> 4) & 0xF;
        let n = (hw1 & 0xF) as usize;
        let d = ((hw2 >> 8) & 0xF) as usize;
        let m = (hw2 & 0xF) as usize;
        if hw2 & 0xF000 != 0xF000 {
            return undef;
        }
        if op1 & 8 == 0 {
            if op2 == 0 {
                // LSL/LSR/ASR/ROR (register) T2 (A7.7.68/70/11/117).
                if bad(d) || bad(n) || bad(m) {
                    return undef;
                }
                let kind = [
                    ShiftType::Lsl,
                    ShiftType::Lsr,
                    ShiftType::Asr,
                    ShiftType::Ror,
                ][((op1 >> 1) & 3) as usize];
                let (r, c) = shift_c(self.r[n], kind, self.r[m] & 0xFF, self.flags.c);
                self.r[d] = r;
                if op1 & 1 != 0 {
                    self.set_nz(r);
                    self.flags.c = c;
                }
                return Ok(());
            }
            if op2 & 8 == 0 || op2 & 4 != 0 || op1 > 5 || bad(d) || bad(m) || n == 13 {
                return undef;
            }
            // SXTAH SXTH UXTAH UXTH SXTAB16 SXTB16 UXTAB16 UXTB16 SXTAB
            // SXTB UXTAB UXTB (A7.7.180-185, A7.7.219-224).
            let rotated = self.r[m].rotate_right(8 * (op2 & 3));
            let add = n != 15;
            let rn = if add { self.r[n] } else { 0 };
            self.r[d] = match op1 {
                0 => rn.wrapping_add(rotated as u16 as i16 as i32 as u32),
                1 => rn.wrapping_add(rotated & 0xFFFF),
                2 | 3 => {
                    let ext = |b: u32| {
                        if op1 == 2 {
                            b as u8 as i8 as i32 as u32
                        } else {
                            b & 0xFF
                        }
                    };
                    let lo = rn.wrapping_add(ext(rotated)) & 0xFFFF;
                    let hi = (rn >> 16).wrapping_add(ext(rotated >> 16)) & 0xFFFF;
                    hi << 16 | lo
                }
                4 => rn.wrapping_add(rotated as u8 as i8 as i32 as u32),
                _ => rn.wrapping_add(rotated & 0xFF),
            };
            return Ok(());
        }
        if bad(d) || bad(n) || bad(m) {
            return undef;
        }
        if op2 & 8 == 0 {
            return self.parallel(pc, insn, op1 & 7, op2, n, d, m);
        }
        if op1 & 0xC != 8 || op2 & 0xC != 8 {
            return undef;
        }
        // A5.3.15 Miscellaneous operations.
        let (rn, rm) = (self.r[n], self.r[m]);
        let consistent = n == m;
        self.r[d] = match (op1 & 3, op2 & 3) {
            (0, k) => {
                // QADD, QDADD, QSUB, QDSUB (A7.7.102/104/109/106).
                let mut q = false;
                let mut operand = i64::from(rn as i32);
                if k & 1 != 0 {
                    let (dbl, s) = signed_sat(2 * operand, 32);
                    q |= s;
                    operand = dbl;
                }
                let sum = if k & 2 == 0 {
                    i64::from(rm as i32) + operand
                } else {
                    i64::from(rm as i32) - operand
                };
                let (r, s) = signed_sat(sum, 32);
                if q || s {
                    self.flags.q = true;
                }
                r as u32
            }
            (1, k) => {
                if !consistent {
                    return undef;
                }
                match k {
                    0 => rm.swap_bytes(),
                    1 => ((rm & 0x00FF_00FF) << 8) | ((rm & 0xFF00_FF00) >> 8),
                    2 => rm.reverse_bits(),
                    _ => (rm as u16).swap_bytes() as i16 as i32 as u32,
                }
            }
            (2, 0) => {
                // SEL (A7.7.127).
                let mut r = 0;
                for i in 0..4 {
                    let src = if self.ge & (1 << i) != 0 { rn } else { rm };
                    r |= src & (0xFF << (8 * i));
                }
                r
            }
            (3, 0) if consistent => rm.leading_zeros(), // CLZ (A7.7.24)
            _ => return undef,
        };
        Ok(())
    }

    /// A5.3.13/A5.3.14 Parallel addition and subtraction, signed and
    /// unsigned, plain (setting GE), saturating (Q prefix) and halving (H).
    #[allow(clippy::too_many_arguments)]
    fn parallel(
        &mut self,
        pc: u32,
        insn: u32,
        op: u32,
        op2: u32,
        n: usize,
        d: usize,
        m: usize,
    ) -> Exec {
        let unsigned = op2 & 4 != 0;
        let prefix = op2 & 3;
        // (lane width, [(n lane, m lane, add)] per result lane).
        let (width, lanes): (u32, &[(u32, u32, bool)]) = match op {
            0 => (8, &[(0, 0, true), (1, 1, true), (2, 2, true), (3, 3, true)]),
            1 => (16, &[(0, 0, true), (1, 1, true)]),
            2 => (16, &[(0, 1, false), (1, 0, true)]), // ASX
            4 => (
                8,
                &[(0, 0, false), (1, 1, false), (2, 2, false), (3, 3, false)],
            ),
            5 => (16, &[(0, 0, false), (1, 1, false)]),
            6 => (16, &[(0, 1, true), (1, 0, false)]), // SAX
            _ => return Err(Stop::Undefined { pc, insn }),
        };
        if prefix == 3 {
            return Err(Stop::Undefined { pc, insn });
        }
        let (rn, rm) = (self.r[n], self.r[m]);
        let mask = (1u32 << width) - 1;
        let lane = |v: u32, i: u32| -> i64 {
            let x = (v >> (width * i)) & mask;
            if unsigned {
                i64::from(x)
            } else {
                i64::from(((x << (32 - width)) as i32) >> (32 - width))
            }
        };
        let mut result = 0u32;
        let mut ge = 0u8;
        for (i, &(a, b, add)) in lanes.iter().enumerate() {
            let x = lane(rn, a);
            let y = lane(rm, b);
            let r = if add { x + y } else { x - y };
            let out = match prefix {
                0 => {
                    let set = if unsigned && add {
                        r >= 1 << width
                    } else {
                        r >= 0
                    };
                    if set {
                        ge |= if width == 8 { 1 << i } else { 3 << (2 * i) };
                    }
                    r
                }
                1 => {
                    if unsigned {
                        unsigned_sat(r, width).0
                    } else {
                        signed_sat(r, width).0
                    }
                }
                _ => r >> 1,
            };
            result |= ((out as u32) & mask) << (width * i as u32);
        }
        self.r[d] = result;
        if prefix == 0 {
            self.ge = ge;
        }
        Ok(())
    }

    /// A5.3.16 Multiply, multiply accumulate, and absolute difference:
    /// MUL, MLA, MLS, SMUL<x><y>, SMLA<x><y>, SMUAD, SMLAD, SMULW<y>,
    /// SMLAW<y>, SMUSD, SMLSD, SMMUL, SMMLA, SMMLS, USAD8, USADA8.
    fn multiply(&mut self, pc: u32, insn: u32) -> Exec {
        let undef = Err(Stop::Undefined { pc, insn });
        let hw1 = insn >> 16;
        let hw2 = insn & 0xFFFF;
        let op1 = (hw1 >> 4) & 7;
        let n = (hw1 & 0xF) as usize;
        let a = (hw2 >> 12) as usize;
        let d = ((hw2 >> 8) & 0xF) as usize;
        let op2 = (hw2 >> 4) & 3;
        let m = (hw2 & 0xF) as usize;
        if hw2 & 0xC0 != 0 || bad(d) || bad(n) || bad(m) || a == 13 {
            return undef;
        }
        let acc = a != 15;
        let (rn, rm) = (self.r[n], self.r[m]);
        let ra = if acc { self.r[a] } else { 0 };
        let ra_s = i64::from(ra as i32);
        let mut q = false;
        let result = match (op1, op2) {
            (0, 0) => rn.wrapping_mul(rm).wrapping_add(ra), // MUL, MLA
            (0, 1) if acc => ra.wrapping_sub(rn.wrapping_mul(rm)), // MLS
            (1, k) => {
                let r = sh(rn, k >> 1) * sh(rm, k & 1) + ra_s;
                q = r != i64::from(r as i32);
                r as u32
            }
            (2 | 4, k) if k & 2 == 0 => {
                let mm = if k & 1 != 0 { rm.rotate_right(16) } else { rm };
                let p1 = sh(rn, 0) * sh(mm, 0);
                let p2 = sh(rn, 1) * sh(mm, 1);
                let r = if op1 == 2 { p1 + p2 } else { p1 - p2 } + ra_s;
                q = r != i64::from(r as i32);
                r as u32
            }
            (3, k) if k & 2 == 0 => {
                let r = i64::from(rn as i32) * sh(rm, k & 1) + (ra_s << 16);
                let v = (r >> 16) as u32;
                q = r >> 16 != i64::from(v as i32);
                v
            }
            (5 | 6, k) if k & 2 == 0 => {
                if op1 == 6 && !acc {
                    return undef;
                }
                let product = i64::from(rn as i32).wrapping_mul(i64::from(rm as i32)) as u64;
                let base = (u64::from(ra)) << 32;
                let mut r = if op1 == 5 {
                    base.wrapping_add(product)
                } else {
                    base.wrapping_sub(product)
                };
                if k & 1 != 0 {
                    r = r.wrapping_add(0x8000_0000);
                }
                (r >> 32) as u32
            }
            (7, 0) => {
                let mut sum = ra;
                for i in 0..4 {
                    let x = (rn >> (8 * i)) & 0xFF;
                    let y = (rm >> (8 * i)) & 0xFF;
                    sum = sum.wrapping_add(x.abs_diff(y));
                }
                sum
            }
            _ => return undef,
        };
        self.r[d] = result;
        if q {
            self.flags.q = true;
        }
        Ok(())
    }

    /// A5.3.17 Long multiply, long multiply accumulate, and divide: SMULL,
    /// UMULL, SMLAL, UMLAL, UMAAL, SMLAL<x><y>, SMLALD, SMLSLD, SDIV, UDIV.
    fn long_multiply_divide(&mut self, pc: u32, insn: u32) -> Exec {
        let undef = Err(Stop::Undefined { pc, insn });
        let hw1 = insn >> 16;
        let hw2 = insn & 0xFFFF;
        let op1 = (hw1 >> 4) & 7;
        let op2 = (hw2 >> 4) & 0xF;
        let n = (hw1 & 0xF) as usize;
        let lo = (hw2 >> 12) as usize;
        let hi = ((hw2 >> 8) & 0xF) as usize;
        let m = (hw2 & 0xF) as usize;
        let (rn, rm) = (self.r[n], self.r[m]);
        if op2 == 0xF && (op1 == 1 || op1 == 3) {
            // SDIV / UDIV (A7.7.125 / A7.7.195). Division by zero gives 0
            // (CCR.DIV_0_TRP = 0, the reset value).
            let d = hi;
            if lo != 15 || bad(d) || bad(n) || bad(m) {
                return undef;
            }
            if rm == 0 && self.exceptions && self.scs.ccr & super::exceptions::ccr::DIV_0_TRP != 0 {
                return Err(Stop::DivideByZero { pc });
            }
            self.r[d] = if rm == 0 {
                0
            } else if op1 == 1 {
                (rn as i32).wrapping_div(rm as i32) as u32
            } else {
                rn / rm
            };
            return Ok(());
        }
        if bad(lo) || bad(hi) || bad(n) || bad(m) || lo == hi {
            return undef;
        }
        let acc = (u64::from(self.r[hi]) << 32) | u64::from(self.r[lo]);
        let s = |v: u32| i64::from(v as i32);
        let result: u64 = match (op1, op2) {
            (0, 0) => s(rn).wrapping_mul(s(rm)) as u64, // SMULL
            (2, 0) => u64::from(rn) * u64::from(rm),    // UMULL
            (4, 0) => (s(rn).wrapping_mul(s(rm)) as u64).wrapping_add(acc), // SMLAL
            (6, 0) => (u64::from(rn) * u64::from(rm)).wrapping_add(acc), // UMLAL
            (6, 6) => {
                // UMAAL (A7.7.203).
                u64::from(rn) * u64::from(rm) + u64::from(self.r[hi]) + u64::from(self.r[lo])
            }
            (4, 8..=0xB) => {
                // SMLAL<x><y> (A7.7.139).
                let p = sh(rn, (op2 >> 1) & 1) * sh(rm, op2 & 1);
                (p as u64).wrapping_add(acc)
            }
            (4 | 5, 0xC | 0xD) => {
                // SMLALD (A7.7.140), SMLSLD (A7.7.145).
                let mm = if op2 & 1 != 0 {
                    rm.rotate_right(16)
                } else {
                    rm
                };
                let p1 = sh(rn, 0) * sh(mm, 0);
                let p2 = sh(rn, 1) * sh(mm, 1);
                let p = if op1 == 4 { p1 + p2 } else { p1 - p2 };
                (p as u64).wrapping_add(acc)
            }
            _ => return undef,
        };
        self.r[lo] = result as u32;
        self.r[hi] = (result >> 32) as u32;
        Ok(())
    }
}

fn wback_conflict(w: bool, n: usize, t: usize, t2: usize) -> bool {
    w && (n == t || n == t2)
}
