//! Cortex-M4 core state and instruction execution.
//!
//! Semantics follow the ARMv7-M Architecture Reference Manual (DDI 0403E),
//! chapter A7. Each instruction's section is cited next to its code so it
//! can be reviewed against the manual.

use crate::alu::{add_with_carry, decode_imm_shift, shift_c, Flags, ShiftType};
use crate::memory::{Bus, Fault, Memory};

/// Why execution stopped. Every case leaves the CPU state as it was
/// before the stopping instruction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stop {
    /// `BKPT #imm` (0xAB is a semihosting call).
    Breakpoint {
        /// Address of the instruction.
        pc: u32,
        /// Immediate operand.
        imm: u8,
    },
    /// `SVC #imm` (exceptions are emulated from milestone M2, part 3).
    Svc {
        /// Address of the instruction.
        pc: u32,
        /// Immediate operand.
        imm: u8,
    },
    /// A permanently undefined or UNPREDICTABLE encoding.
    Undefined {
        /// Address of the instruction.
        pc: u32,
        /// The instruction (first halfword in the high 16 bits for 32-bit).
        insn: u32,
    },
    /// An encoding this version of the emulator does not implement yet.
    /// Replay stops instead of guessing.
    Unimplemented {
        /// Address of the instruction.
        pc: u32,
        /// The instruction (first halfword in the high 16 bits for 32-bit).
        insn: u32,
    },
    /// A memory access nothing answered.
    BusFault {
        /// Address of the instruction.
        pc: u32,
        /// The access.
        fault: Fault,
    },
    /// Interworking branch to an address with bit 0 clear (INVSTATE).
    InvalidState {
        /// Address of the instruction.
        pc: u32,
        /// Target address.
        target: u32,
    },
    /// Branch to an `EXC_RETURN` value (exception return, part 3).
    ExceptionReturn {
        /// Address of the instruction.
        pc: u32,
        /// The `EXC_RETURN` value.
        value: u32,
    },
}

/// The core: registers, flags and memory.
#[derive(Debug, Clone)]
pub struct Cpu {
    /// r0-r15. `r[15]` is the address of the next instruction to execute.
    pub r: [u32; 16],
    /// APSR condition flags.
    pub flags: Flags,
    /// ITSTATE (`IT[7:0]`).
    pub it: u8,
    /// IPSR: current exception number (0 = thread mode).
    pub ipsr: u32,
    /// PRIMASK.
    pub primask: bool,
    /// RAM and flash.
    pub memory: Memory,
    /// Instructions executed (including ones skipped by IT).
    pub instructions: u64,
}

type Exec = Result<(), Stop>;

impl Cpu {
    /// A core with the given memory, all registers zero.
    #[must_use]
    pub fn new(memory: Memory) -> Self {
        Cpu {
            r: [0; 16],
            flags: Flags::default(),
            it: 0,
            ipsr: 0,
            primask: false,
            memory,
            instructions: 0,
        }
    }

    /// Reset: SP and PC from the vector table at `vtor`, LR = 0xFFFFFFFF,
    /// as on the real core (B1.5.5, `TakeReset`).
    ///
    /// # Errors
    /// If the vector table is not in memory.
    pub fn reset(&mut self, vtor: u32) -> Result<(), Fault> {
        let missing = |address| Fault {
            address,
            size: 4,
            write: false,
        };
        let sp = self.memory.read(vtor, 4).ok_or(missing(vtor))?;
        let pc = self.memory.read(vtor + 4, 4).ok_or(missing(vtor + 4))?;
        self.r = [0; 16];
        self.r[13] = sp & !3;
        self.r[14] = 0xFFFF_FFFF;
        self.r[15] = pc & !1;
        self.flags = Flags::default();
        self.it = 0;
        self.ipsr = 0;
        self.primask = false;
        Ok(())
    }

    /// xPSR as the architecture presents it (flags, T bit, IT bits, IPSR).
    #[must_use]
    pub fn xpsr(&self) -> u32 {
        let it = u32::from(self.it);
        self.flags.to_bits() | (1 << 24) | ((it & 3) << 25) | ((it >> 2) << 10) | self.ipsr
    }

    fn in_it_block(&self) -> bool {
        self.it & 0xF != 0
    }

    fn it_advance(&mut self) {
        if self.it & 7 == 0 {
            self.it = 0;
        } else {
            self.it = (self.it & 0xE0) | ((self.it << 1) & 0x1F);
        }
    }

    fn read(&mut self, bus: &mut dyn Bus, pc: u32, address: u32, size: u8) -> Result<u32, Stop> {
        if let Some(v) = self.memory.read(address, size) {
            return Ok(v);
        }
        bus.read(address, size)
            .map_err(|fault| Stop::BusFault { pc, fault })
    }

    fn write(&mut self, bus: &mut dyn Bus, pc: u32, address: u32, size: u8, value: u32) -> Exec {
        match self.memory.write(address, size, value) {
            Some(true) => Ok(()),
            Some(false) => Err(Stop::BusFault {
                pc,
                fault: Fault {
                    address,
                    size,
                    write: true,
                },
            }),
            None => bus
                .write(address, size, value)
                .map_err(|fault| Stop::BusFault { pc, fault }),
        }
    }

    /// Execute one instruction.
    ///
    /// # Errors
    /// [`Stop`] if the instruction cannot be executed; the state is then
    /// unchanged.
    pub fn step(&mut self, bus: &mut dyn Bus) -> Exec {
        let pc = self.r[15];
        let hw1 = self.memory.read(pc, 2).ok_or(Stop::BusFault {
            pc,
            fault: Fault {
                address: pc,
                size: 2,
                write: false,
            },
        })? as u16;
        if is_32bit(hw1) {
            let hw2 = self.memory.read(pc + 2, 2).ok_or(Stop::BusFault {
                pc,
                fault: Fault {
                    address: pc + 2,
                    size: 2,
                    write: false,
                },
            })? as u16;
            return Err(Stop::Unimplemented {
                pc,
                insn: (u32::from(hw1) << 16) | u32::from(hw2),
            });
        }
        let saved = (self.r, self.flags, self.it, self.primask);
        let is_it = hw1 & 0xFF00 == 0xBF00 && hw1 & 0xF != 0;
        let in_it = self.in_it_block();
        if in_it && not_allowed_in_it(hw1, self.it & 0xF == 0x8) {
            // UNPREDICTABLE whether or not the condition passes.
            return Err(Stop::Undefined {
                pc,
                insn: u32::from(hw1),
            });
        }
        let passed = !in_it || self.flags.condition_passed(u32::from(self.it >> 4));
        self.r[15] = pc + 2;
        let result = if passed {
            self.exec16(bus, pc, hw1, in_it)
        } else {
            Ok(())
        };
        if let Err(stop) = result {
            (self.r, self.flags, self.it, self.primask) = saved;
            self.r[15] = pc;
            return Err(stop);
        }
        if in_it && !is_it {
            self.it_advance();
        }
        self.instructions += 1;
        Ok(())
    }

    /// Register read as an operand: PC reads as the instruction address + 4.
    fn reg(&self, n: usize, pc: u32) -> u32 {
        if n == 15 {
            pc.wrapping_add(4)
        } else {
            self.r[n]
        }
    }

    fn set_nz(&mut self, v: u32) {
        self.flags.n = v >> 31 != 0;
        self.flags.z = v == 0;
    }

    /// `BXWritePC` / `LoadWritePC` (A2.3.1).
    fn bx_write_pc(&mut self, pc: u32, target: u32) -> Exec {
        if self.ipsr != 0 && target >> 28 == 0xF {
            return Err(Stop::ExceptionReturn { pc, value: target });
        }
        if target & 1 == 0 {
            return Err(Stop::InvalidState { pc, target });
        }
        self.r[15] = target & !1;
        Ok(())
    }

    #[allow(clippy::too_many_lines)]
    fn exec16(&mut self, bus: &mut dyn Bus, pc: u32, hw: u16, in_it: bool) -> Exec {
        let h = u32::from(hw);
        let setflags = !in_it;
        let rd = (h & 7) as usize;
        let rn = ((h >> 3) & 7) as usize;
        let rm6 = ((h >> 6) & 7) as usize;
        match h >> 10 {
            // A5.2.1 Shift (immediate), add, subtract, move, compare.
            0x00..=0x0F => {
                let op = (h >> 9) & 0x1F;
                match op {
                    0x00..=0x0B => {
                        // LSL/LSR/ASR (immediate), A7.7.67/A7.7.69/A7.7.10.
                        // LSL #0 is MOVS Rd, Rm (A7.7.76 T2): carry unchanged.
                        let (kind, amount) = decode_imm_shift((h >> 11) & 3, (h >> 6) & 0x1F);
                        let (r, c) = shift_c(self.r[rn], kind, amount, self.flags.c);
                        self.r[rd] = r;
                        if setflags {
                            self.set_nz(r);
                            self.flags.c = c;
                        }
                    }
                    0x0C..=0x0F => {
                        // ADD/SUB register and 3-bit immediate (A7.7.3/4, A7.7.174/175).
                        let operand = if op & 2 == 0 {
                            self.r[rm6]
                        } else {
                            (h >> 6) & 7
                        };
                        let (r, c, v) = if op & 1 == 0 {
                            add_with_carry(self.r[rn], operand, false)
                        } else {
                            add_with_carry(self.r[rn], !operand, true)
                        };
                        self.r[rd] = r;
                        if setflags {
                            self.set_nz(r);
                            self.flags.c = c;
                            self.flags.v = v;
                        }
                    }
                    _ => {
                        // MOV/CMP/ADD/SUB 8-bit immediate (A7.7.75, A7.7.27, A7.7.3, A7.7.174).
                        let rdn = ((h >> 8) & 7) as usize;
                        let imm = h & 0xFF;
                        match (op >> 2) & 3 {
                            0 => {
                                self.r[rdn] = imm;
                                if setflags {
                                    self.set_nz(imm);
                                }
                            }
                            1 => {
                                let (r, c, v) = add_with_carry(self.r[rdn], !imm, true);
                                self.set_nz(r);
                                self.flags.c = c;
                                self.flags.v = v;
                            }
                            k => {
                                let (r, c, v) = if k == 2 {
                                    add_with_carry(self.r[rdn], imm, false)
                                } else {
                                    add_with_carry(self.r[rdn], !imm, true)
                                };
                                self.r[rdn] = r;
                                if setflags {
                                    self.set_nz(r);
                                    self.flags.c = c;
                                    self.flags.v = v;
                                }
                            }
                        }
                    }
                }
            }
            // A5.2.2 Data processing (register).
            0x10 => self.data_processing16(h, rd, rn, setflags),
            // A5.2.3 Special data instructions and branch and exchange.
            0x11 => return self.special16(pc, h),
            // LDR (literal) A7.7.43.
            0x12 | 0x13 => {
                let rt = ((h >> 8) & 7) as usize;
                let addr = (pc.wrapping_add(4) & !3).wrapping_add((h & 0xFF) << 2);
                self.r[rt] = self.read(bus, pc, addr, 4)?;
            }
            // A5.2.4 Load/store single data item.
            0x14..=0x27 => return self.load_store16(bus, pc, h),
            // ADR A7.7.7.
            0x28 | 0x29 => {
                let rd8 = ((h >> 8) & 7) as usize;
                self.r[rd8] = (pc.wrapping_add(4) & !3).wrapping_add((h & 0xFF) << 2);
            }
            // ADD (SP plus immediate) A7.7.5 T1.
            0x2A | 0x2B => {
                let rd8 = ((h >> 8) & 7) as usize;
                self.r[rd8] = self.r[13].wrapping_add((h & 0xFF) << 2);
            }
            // A5.2.5 Miscellaneous 16-bit instructions.
            0x2C..=0x2F => return self.misc16(bus, pc, h),
            // STM / LDM A7.7.157, A7.7.41.
            0x30..=0x33 => {
                let rn8 = ((h >> 8) & 7) as usize;
                let list = h & 0xFF;
                if list == 0 {
                    return Err(Stop::Undefined { pc, insn: h });
                }
                let load = h & 0x0800 != 0;
                let base = self.r[rn8];
                let mut addr = base;
                if !load && list & (1 << rn8) != 0 && list & ((1 << rn8) - 1) != 0 {
                    return Err(Stop::Undefined { pc, insn: h }); // UNPREDICTABLE
                }
                for i in 0..8 {
                    if list & (1 << i) != 0 {
                        if load {
                            self.r[i] = self.read(bus, pc, addr, 4)?;
                        } else {
                            let v = self.r[i];
                            self.write(bus, pc, addr, 4, v)?;
                        }
                        addr = addr.wrapping_add(4);
                    }
                }
                if !load || list & (1 << rn8) == 0 {
                    self.r[rn8] = base.wrapping_add(4 * list.count_ones());
                }
            }
            // A5.2.6 Conditional branch, and Supervisor Call.
            0x34..=0x37 => {
                let cond = (h >> 8) & 0xF;
                match cond {
                    0xE => return Err(Stop::Undefined { pc, insn: h }), // UDF
                    0xF => {
                        return Err(Stop::Svc {
                            pc,
                            imm: (h & 0xFF) as u8,
                        })
                    }
                    _ => {
                        if in_it {
                            return Err(Stop::Undefined { pc, insn: h });
                        }
                        if self.flags.condition_passed(cond) {
                            let imm = ((h & 0xFF) as u8 as i8 as i32) << 1;
                            self.r[15] = pc.wrapping_add(4).wrapping_add(imm as u32);
                        }
                    }
                }
            }
            // B (unconditional) A7.7.12 T2.
            0x38 | 0x39 => {
                if in_it && self.it & 0xF != 0x8 {
                    return Err(Stop::Undefined { pc, insn: h }); // only last in IT
                }
                let imm = (((h & 0x7FF) << 21) as i32) >> 20;
                self.r[15] = pc.wrapping_add(4).wrapping_add(imm as u32);
            }
            _ => unreachable!("32-bit encodings are handled before exec16"),
        }
        Ok(())
    }

    /// A5.2.2: AND EOR LSL LSR ASR ADC SBC ROR TST RSB CMP CMN ORR MUL BIC MVN.
    fn data_processing16(&mut self, h: u32, rdn: usize, rm: usize, setflags: bool) {
        let a = self.r[rdn];
        let b = self.r[rm];
        let c_in = self.flags.c;
        let op = (h >> 6) & 0xF;
        // (result, write result, carry, overflow); None = flag unchanged.
        let (result, write, carry, overflow): (u32, bool, Option<bool>, Option<bool>) = match op {
            0x0 => (a & b, true, None, None),
            0x1 => (a ^ b, true, None, None),
            0x2 | 0x3 | 0x4 | 0x7 => {
                let kind = [
                    ShiftType::Lsl,
                    ShiftType::Lsr,
                    ShiftType::Asr,
                    ShiftType::Ror,
                ][[0usize, 1, 2, 0, 0, 3][(op - 2) as usize]];
                let (r, c) = shift_c(a, kind, b & 0xFF, c_in);
                (r, true, Some(c), None)
            }
            0x5 | 0x6 => {
                let (r, c, v) = if op == 5 {
                    add_with_carry(a, b, c_in)
                } else {
                    add_with_carry(a, !b, c_in)
                };
                (r, true, Some(c), Some(v))
            }
            0x8 => (a & b, false, None, None),
            0x9 => {
                let (r, c, v) = add_with_carry(!b, 0, true); // RSB Rd, Rn, #0
                (r, true, Some(c), Some(v))
            }
            0xA => {
                let (r, c, v) = add_with_carry(a, !b, true);
                (r, false, Some(c), Some(v))
            }
            0xB => {
                let (r, c, v) = add_with_carry(a, b, false);
                (r, false, Some(c), Some(v))
            }
            0xC => (a | b, true, None, None),
            0xD => (a.wrapping_mul(b), true, None, None),
            0xE => (a & !b, true, None, None),
            _ => (!b, true, None, None),
        };
        // RSB's operand is Rn = bits 5:3 (rm here), result to Rd.
        if write {
            self.r[rdn] = result;
        }
        let always = matches!(op, 0x8 | 0xA | 0xB);
        if setflags || always {
            self.set_nz(result);
            if let Some(c) = carry {
                self.flags.c = c;
            }
            if let Some(v) = overflow {
                self.flags.v = v;
            }
        }
    }

    /// A5.2.3: ADD/CMP/MOV (high registers), BX, BLX.
    fn special16(&mut self, pc: u32, h: u32) -> Exec {
        let rm = ((h >> 3) & 0xF) as usize;
        let rdn = (((h >> 4) & 8) | (h & 7)) as usize;
        match (h >> 6) & 0xF {
            0x0..=0x3 => {
                // ADD (register) T2, A7.7.4. No flags.
                if rdn == 15 && rm == 15 {
                    return Err(Stop::Undefined { pc, insn: h });
                }
                let r = self.reg(rdn, pc).wrapping_add(self.reg(rm, pc));
                if rdn == 15 {
                    if self.in_it_block() && self.it & 0xF != 0x8 {
                        return Err(Stop::Undefined { pc, insn: h });
                    }
                    self.r[15] = r & !1;
                } else {
                    self.r[rdn] = r;
                }
            }
            0x4 => return Err(Stop::Undefined { pc, insn: h }),
            0x5..=0x7 => {
                // CMP (register) T2, A7.7.28.
                if rdn < 8 && rm < 8 || rdn == 15 || rm == 15 {
                    return Err(Stop::Undefined { pc, insn: h });
                }
                let (r, c, v) = add_with_carry(self.r[rdn], !self.r[rm], true);
                self.set_nz(r);
                self.flags.c = c;
                self.flags.v = v;
            }
            0x8..=0xB => {
                // MOV (register) T1, A7.7.77. No flags.
                let v = self.reg(rm, pc);
                if rdn == 15 {
                    if self.in_it_block() && self.it & 0xF != 0x8 {
                        return Err(Stop::Undefined { pc, insn: h });
                    }
                    self.r[15] = v & !1;
                } else {
                    self.r[rdn] = v;
                }
            }
            op => {
                // BX / BLX (register), A7.7.20 / A7.7.19.
                if h & 7 != 0 || (op >= 0xE && rm == 15) {
                    return Err(Stop::Undefined { pc, insn: h });
                }
                if self.in_it_block() && self.it & 0xF != 0x8 {
                    return Err(Stop::Undefined { pc, insn: h });
                }
                let target = self.reg(rm, pc);
                if op >= 0xE {
                    if target & 1 == 0 {
                        return Err(Stop::InvalidState { pc, target });
                    }
                    self.r[14] = (pc + 2) | 1;
                }
                return self.bx_write_pc(pc, target);
            }
        }
        Ok(())
    }

    /// A5.2.4: load/store single data item.
    fn load_store16(&mut self, bus: &mut dyn Bus, pc: u32, h: u32) -> Exec {
        let rt = (h & 7) as usize;
        let rn = ((h >> 3) & 7) as usize;
        let imm5 = (h >> 6) & 0x1F;
        // (is_load, size, sign_extend, address, rt)
        let (load, size, sign, addr, rt) = match h >> 11 {
            0x0A | 0x0B => {
                let opb = (h >> 9) & 7;
                let addr = self.r[rn].wrapping_add(self.r[((h >> 6) & 7) as usize]);
                let (l, s, x) = [
                    (false, 4, false),
                    (false, 2, false),
                    (false, 1, false),
                    (true, 1, true),
                    (true, 4, false),
                    (true, 2, false),
                    (true, 1, false),
                    (true, 2, true),
                ][opb as usize];
                (l, s, x, addr, rt)
            }
            0x0C | 0x0D => (
                h & 0x0800 != 0,
                4,
                false,
                self.r[rn].wrapping_add(imm5 << 2),
                rt,
            ),
            0x0E | 0x0F => (h & 0x0800 != 0, 1, false, self.r[rn].wrapping_add(imm5), rt),
            0x10 | 0x11 => (
                h & 0x0800 != 0,
                2,
                false,
                self.r[rn].wrapping_add(imm5 << 1),
                rt,
            ),
            _ => {
                // 1001x: STR/LDR (SP-relative), imm8 * 4.
                let rt8 = ((h >> 8) & 7) as usize;
                (
                    h & 0x0800 != 0,
                    4,
                    false,
                    self.r[13].wrapping_add((h & 0xFF) << 2),
                    rt8,
                )
            }
        };
        if load {
            let v = self.read(bus, pc, addr, size)?;
            self.r[rt] = match (sign, size) {
                (true, 1) => v as u8 as i8 as i32 as u32,
                (true, _) => v as u16 as i16 as i32 as u32,
                _ => v,
            };
        } else {
            let v = self.r[rt];
            self.write(bus, pc, addr, size, v)?;
        }
        Ok(())
    }

    /// A5.2.5: miscellaneous 16-bit instructions.
    fn misc16(&mut self, bus: &mut dyn Bus, pc: u32, h: u32) -> Exec {
        let rd = (h & 7) as usize;
        let rm = ((h >> 3) & 7) as usize;
        match (h >> 5) & 0x7F {
            0x00..=0x03 => self.r[13] = self.r[13].wrapping_add((h & 0x7F) << 2), // ADD SP, #imm7
            0x04..=0x07 => self.r[13] = self.r[13].wrapping_sub((h & 0x7F) << 2), // SUB SP, #imm7
            0x10..=0x11 => self.r[rd] = self.r[rm] as u16 as i16 as i32 as u32,   // SXTH
            0x12..=0x13 => self.r[rd] = self.r[rm] as u8 as i8 as i32 as u32,     // SXTB
            0x14..=0x15 => self.r[rd] = self.r[rm] & 0xFFFF,                      // UXTH
            0x16..=0x17 => self.r[rd] = self.r[rm] & 0xFF,                        // UXTB
            0x20..=0x2F => {
                // PUSH A7.7.99 T1: registers in ascending order at lower addresses.
                let list = (h & 0xFF) | ((h & 0x100) << 6); // bit 8 -> LR (bit 14)
                if list == 0 {
                    return Err(Stop::Undefined { pc, insn: h });
                }
                let mut addr = self.r[13].wrapping_sub(4 * list.count_ones());
                let new_sp = addr;
                for i in 0..15 {
                    if list & (1 << i) != 0 {
                        let v = self.r[i];
                        self.write(bus, pc, addr, 4, v)?;
                        addr = addr.wrapping_add(4);
                    }
                }
                self.r[13] = new_sp;
            }
            0x33 => {
                // CPS A7.7.29: im = bit 4; affects PRIMASK (I, bit 1). FAULTMASK
                // (F, bit 0) is not modelled yet.
                if h & 0xF != 0x2 && h & 0xF != 0x3 && h & 0xF != 0x1 {
                    return Err(Stop::Undefined { pc, insn: h });
                }
                if self.in_it_block() {
                    return Err(Stop::Undefined { pc, insn: h });
                }
                if h & 0x2 != 0 {
                    self.primask = h & 0x10 != 0;
                }
                if h & 0x1 != 0 {
                    return Err(Stop::Unimplemented { pc, insn: h });
                }
            }
            0x50 | 0x51 => self.r[rd] = self.r[rm].swap_bytes(), // REV 101000x
            0x52 | 0x53 => {
                // REV16 101001x
                let v = self.r[rm];
                self.r[rd] = ((v & 0x00FF_00FF) << 8) | ((v & 0xFF00_FF00) >> 8);
            }
            0x56 | 0x57 => self.r[rd] = (self.r[rm] as u16).swap_bytes() as i16 as i32 as u32, // REVSH 101011x
            0x60..=0x6F => {
                // POP A7.7.98 T1.
                let list = (h & 0xFF) | ((h & 0x100) << 7); // bit 8 -> PC (bit 15)
                if list == 0 {
                    return Err(Stop::Undefined { pc, insn: h });
                }
                if list & 0x8000 != 0 && self.in_it_block() && self.it & 0xF != 0x8 {
                    return Err(Stop::Undefined { pc, insn: h });
                }
                let mut addr = self.r[13];
                let mut new_pc = None;
                let mut values = [0u32; 16];
                for (i, slot) in values.iter_mut().enumerate() {
                    if list & (1 << i) != 0 {
                        *slot = self.read(bus, pc, addr, 4)?;
                        addr = addr.wrapping_add(4);
                    }
                }
                for (i, v) in values.iter().enumerate().take(8) {
                    if list & (1 << i) != 0 {
                        self.r[i] = *v;
                    }
                }
                if list & 0x8000 != 0 {
                    new_pc = Some(values[15]);
                }
                let old_sp = self.r[13];
                self.r[13] = addr;
                if let Some(target) = new_pc {
                    if let Err(e) = self.bx_write_pc(pc, target) {
                        self.r[13] = old_sp;
                        return Err(e);
                    }
                }
            }
            0x70..=0x77 => {
                return Err(Stop::Breakpoint {
                    pc,
                    imm: (h & 0xFF) as u8,
                })
            } // BKPT
            0x78..=0x7F => {
                if h & 0xF != 0 {
                    // IT A7.7.38.
                    let firstcond = (h >> 4) & 0xF;
                    if firstcond == 0xF || (firstcond == 0xE && (h & 0xF).count_ones() != 1) {
                        return Err(Stop::Undefined { pc, insn: h });
                    }
                    if self.in_it_block() {
                        return Err(Stop::Undefined { pc, insn: h });
                    }
                    self.it = (h & 0xFF) as u8;
                } else {
                    // NOP, YIELD, WFE, WFI, SEV: no architectural effect here.
                    // During replay, interrupts are injected from the recording.
                    if (h >> 4) & 0xF > 4 {
                        return Err(Stop::Undefined { pc, insn: h });
                    }
                }
            }
            _ => {
                // CBZ / CBNZ A7.7.21: 1011 op 0 i 1 imm5 Rn.
                if h & 0x0500 == 0x0100 {
                    if self.in_it_block() {
                        return Err(Stop::Undefined { pc, insn: h });
                    }
                    let nonzero = h & 0x0800 != 0;
                    let offset = (((h >> 9) & 1) << 6) | (((h >> 3) & 0x1F) << 1);
                    if (self.r[rd] != 0) == nonzero {
                        self.r[15] = pc.wrapping_add(4).wrapping_add(offset);
                    }
                } else {
                    return Err(Stop::Undefined { pc, insn: h });
                }
            }
        }
        Ok(())
    }
}

/// 16-bit instructions that may not appear in an IT block at all, or only
/// as its last instruction (A7.3.3).
fn not_allowed_in_it(hw: u16, last: bool) -> bool {
    let h = u32::from(hw);
    let never = (h & 0xFF00 == 0xBF00 && h & 0xF != 0)        // IT
        || (h >> 12 == 0xD && (h >> 8) & 0xE != 0xE)           // B<cond>
        || (h & 0xF500 == 0xB100)                              // CBZ/CBNZ
        || (h & 0xFFEC == 0xB660); // CPS
    let branch = h >> 11 == 0x1C                               // B
        || (h & 0xFF00 == 0x4700)                              // BX/BLX
        || (h & 0xFF00 == 0xBD00)                              // POP {.., pc}
        || (h & 0xFF87 == 0x4487)                              // ADD pc, Rm
        || (h & 0xFF87 == 0x4687); // MOV pc, Rm
    never || (branch && !last)
}

/// True if the halfword starts a 32-bit Thumb-2 instruction.
#[must_use]
pub fn is_32bit(hw1: u16) -> bool {
    matches!(hw1 >> 11, 0x1D..=0x1F)
}
