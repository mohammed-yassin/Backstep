//! Arithmetic helpers with the exact flag semantics of the ARMv7-M
//! Architecture Reference Manual (pseudocode functions `AddWithCarry`,
//! `Shift_C`, `ConditionPassed`).

/// Result of `AddWithCarry(x, y, carry_in)`: (result, carry, overflow).
#[must_use]
pub fn add_with_carry(x: u32, y: u32, carry_in: bool) -> (u32, bool, bool) {
    let unsigned = u64::from(x) + u64::from(y) + u64::from(carry_in);
    let result = (unsigned & 0xFFFF_FFFF) as u32;
    let carry = unsigned > 0xFFFF_FFFF;
    let signed = i64::from(x as i32) + i64::from(y as i32) + i64::from(carry_in);
    let overflow = signed != i64::from(result as i32);
    (result, carry, overflow)
}

/// Shift types, as encoded in instructions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShiftType {
    /// Logical shift left.
    Lsl,
    /// Logical shift right.
    Lsr,
    /// Arithmetic shift right.
    Asr,
    /// Rotate right.
    Ror,
    /// Rotate right by one through carry.
    Rrx,
}

/// `Shift_C(value, type, amount, carry_in)`: (result, `carry_out`).
///
/// `amount` is the full shift amount (for register-specified shifts the
/// bottom byte of the register, which may exceed 31).
#[must_use]
pub fn shift_c(value: u32, kind: ShiftType, amount: u32, carry_in: bool) -> (u32, bool) {
    if amount == 0 && kind != ShiftType::Rrx {
        return (value, carry_in);
    }
    match kind {
        ShiftType::Lsl => match amount {
            1..=31 => (value << amount, (value >> (32 - amount)) & 1 != 0),
            32 => (0, value & 1 != 0),
            _ => (0, false),
        },
        ShiftType::Lsr => match amount {
            1..=31 => (value >> amount, (value >> (amount - 1)) & 1 != 0),
            32 => (0, value >> 31 != 0),
            _ => (0, false),
        },
        ShiftType::Asr => {
            let v = value as i32;
            if amount >= 32 {
                let r = (v >> 31) as u32;
                (r, r & 1 != 0)
            } else {
                ((v >> amount) as u32, (value >> (amount - 1)) & 1 != 0)
            }
        }
        ShiftType::Ror => {
            let m = amount % 32;
            let r = value.rotate_right(m);
            (r, r >> 31 != 0)
        }
        ShiftType::Rrx => ((u32::from(carry_in) << 31) | (value >> 1), value & 1 != 0),
    }
}

/// `DecodeImmShift(type, imm5)` for immediate shifts: (type, amount).
#[must_use]
pub fn decode_imm_shift(kind: u32, imm5: u32) -> (ShiftType, u32) {
    match kind & 3 {
        0 => (ShiftType::Lsl, imm5),
        1 => (ShiftType::Lsr, if imm5 == 0 { 32 } else { imm5 }),
        2 => (ShiftType::Asr, if imm5 == 0 { 32 } else { imm5 }),
        _ => {
            if imm5 == 0 {
                (ShiftType::Rrx, 1)
            } else {
                (ShiftType::Ror, imm5)
            }
        }
    }
}

/// Condition flags.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)]
pub struct Flags {
    /// Negative.
    pub n: bool,
    /// Zero.
    pub z: bool,
    /// Carry.
    pub c: bool,
    /// Overflow.
    pub v: bool,
    /// Saturation (sticky).
    pub q: bool,
}

impl Flags {
    /// `ConditionPassed()` for a 4-bit condition code.
    #[must_use]
    pub fn condition_passed(self, cond: u32) -> bool {
        let base = match cond >> 1 {
            0 => self.z,
            1 => self.c,
            2 => self.n,
            3 => self.v,
            4 => self.c && !self.z,
            5 => self.n == self.v,
            6 => self.n == self.v && !self.z,
            _ => true,
        };
        if cond & 1 == 1 && cond != 0xF {
            !base
        } else {
            base
        }
    }

    /// Bits 31:27 of xPSR.
    #[must_use]
    pub fn to_bits(self) -> u32 {
        (u32::from(self.n) << 31)
            | (u32::from(self.z) << 30)
            | (u32::from(self.c) << 29)
            | (u32::from(self.v) << 28)
            | (u32::from(self.q) << 27)
    }

    /// Flags from bits 31:27 of a value.
    #[must_use]
    pub fn from_bits(bits: u32) -> Self {
        Flags {
            n: bits & (1 << 31) != 0,
            z: bits & (1 << 30) != 0,
            c: bits & (1 << 29) != 0,
            v: bits & (1 << 28) != 0,
            q: bits & (1 << 27) != 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_with_carry_flags() {
        assert_eq!(add_with_carry(1, 2, false), (3, false, false));
        assert_eq!(add_with_carry(0xFFFF_FFFF, 1, false), (0, true, false));
        assert_eq!(
            add_with_carry(0x7FFF_FFFF, 1, false),
            (0x8000_0000, false, true)
        );
        assert_eq!(
            add_with_carry(0x8000_0000, 0x8000_0000, false),
            (0, true, true)
        );
        // Subtraction x - y = x + !y + 1.
        assert_eq!(add_with_carry(5, !5, true), (0, true, false));
        assert_eq!(add_with_carry(3, !5, true), (0xFFFF_FFFE, false, false));
        assert_eq!(
            add_with_carry(0x8000_0000, !1, true),
            (0x7FFF_FFFF, true, true)
        );
    }

    #[test]
    fn shifts_follow_the_architecture() {
        use ShiftType::*;
        assert_eq!(shift_c(0x8000_0001, Lsl, 1, false), (2, true));
        assert_eq!(shift_c(1, Lsl, 32, false), (0, true));
        assert_eq!(shift_c(1, Lsl, 33, true), (0, false));
        assert_eq!(shift_c(0x8000_0000, Lsr, 32, false), (0, true));
        assert_eq!(shift_c(3, Lsr, 1, false), (1, true));
        assert_eq!(shift_c(0x8000_0000, Asr, 40, false), (0xFFFF_FFFF, true));
        assert_eq!(shift_c(0x8000_0000, Asr, 4, false), (0xF800_0000, false));
        assert_eq!(shift_c(0x1234_5678, Ror, 32, false), (0x1234_5678, false));
        assert_eq!(shift_c(1, Ror, 1, false), (0x8000_0000, true));
        assert_eq!(shift_c(3, Rrx, 1, true), (0x8000_0001, true));
        assert_eq!(shift_c(7, Lsl, 0, true), (7, true));
        assert_eq!(decode_imm_shift(1, 0), (Lsr, 32));
        assert_eq!(decode_imm_shift(3, 0), (Rrx, 1));
    }

    #[test]
    fn conditions() {
        let f = Flags {
            n: false,
            z: true,
            c: true,
            v: false,
            q: false,
        };
        assert!(f.condition_passed(0)); // EQ
        assert!(!f.condition_passed(1)); // NE
        assert!(f.condition_passed(2)); // CS
        assert!(!f.condition_passed(8)); // HI: C && !Z
        assert!(f.condition_passed(9)); // LS
        assert!(f.condition_passed(10)); // GE: N == V
        assert!(!f.condition_passed(12)); // GT
        assert!(f.condition_passed(13)); // LE
        assert!(f.condition_passed(14)); // AL
        assert_eq!(Flags::from_bits(f.to_bits()), f);
    }
}
