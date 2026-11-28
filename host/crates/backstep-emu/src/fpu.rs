//! IEEE 754 single-precision arithmetic as the Cortex-M4 floating-point
//! unit (FPv4-SP) computes it: every rounding mode, flush-to-zero, default
//! NaN, and the cumulative exception flags of FPSCR (ARMv7-M Architecture
//! Reference Manual, A2.5, and the pseudocode of `FPAdd`, `FPMul`,
//! `FPMulAdd`, `FPDiv`, `FPSqrt`, `FPCompare`, `FPToFixed`, `FixedToFP`,
//! `FPHalfToSingle` and `FPSingleToHalf`).
//!
//! Values are bit patterns (`u32`, `u16`); nothing here uses the host's
//! floating point, so results are the same on every host.

/// FPSCR bits.
pub mod fpscr {
    /// Invalid operation (cumulative).
    pub const IOC: u32 = 1 << 0;
    /// Division by zero.
    pub const DZC: u32 = 1 << 1;
    /// Overflow.
    pub const OFC: u32 = 1 << 2;
    /// Underflow.
    pub const UFC: u32 = 1 << 3;
    /// Inexact.
    pub const IXC: u32 = 1 << 4;
    /// Input denormal (flushed to zero).
    pub const IDC: u32 = 1 << 7;
    /// Rounding mode, bits 23:22.
    pub const RMODE_SHIFT: u32 = 22;
    /// Flush-to-zero.
    pub const FZ: u32 = 1 << 24;
    /// Default NaN.
    pub const DN: u32 = 1 << 25;
    /// Alternative half-precision format.
    pub const AHP: u32 = 1 << 26;
    /// The bits software can write on an FPv4-SP core.
    pub const WRITABLE: u32 = 0xF7C0_009F;
}

/// The default NaN.
pub const DEFAULT_NAN: u32 = 0x7FC0_0000;
const QUIET: u32 = 0x0040_0000;

/// A rounding mode (FPSCR.RMode).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rounding {
    /// Round to nearest, ties to even (RN).
    Nearest,
    /// Towards plus infinity (RP).
    PlusInf,
    /// Towards minus infinity (RM).
    MinusInf,
    /// Towards zero (RZ).
    Zero,
}

/// The FPSCR controls an operation uses, and the flags it raises.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Env {
    /// Rounding.
    pub rounding: Rounding,
    /// Flush denormal inputs and results to zero.
    pub fz: bool,
    /// Return the default NaN instead of propagating NaNs.
    pub dn: bool,
    /// Alternative half-precision (VCVTB/VCVTT).
    pub ahp: bool,
    /// Exception flags raised (FPSCR bit positions), to be ORed into
    /// FPSCR.
    pub flags: u32,
}

impl Env {
    /// The controls of `fpscr` (no flags raised yet).
    #[must_use]
    pub fn from_fpscr(fpscr: u32) -> Self {
        Env {
            rounding: match (fpscr >> fpscr::RMODE_SHIFT) & 3 {
                0 => Rounding::Nearest,
                1 => Rounding::PlusInf,
                2 => Rounding::MinusInf,
                _ => Rounding::Zero,
            },
            fz: fpscr & fpscr::FZ != 0,
            dn: fpscr & fpscr::DN != 0,
            ahp: fpscr & fpscr::AHP != 0,
            flags: 0,
        }
    }
}

fn sign(x: u32) -> bool {
    x >> 31 != 0
}

fn exp_field(x: u32) -> u32 {
    (x >> 23) & 0xFF
}

fn frac(x: u32) -> u32 {
    x & 0x007F_FFFF
}

fn is_nan(x: u32) -> bool {
    exp_field(x) == 0xFF && frac(x) != 0
}

fn is_snan(x: u32) -> bool {
    is_nan(x) && x & QUIET == 0
}

fn is_inf(x: u32) -> bool {
    x & 0x7FFF_FFFF == 0x7F80_0000
}

fn is_zero(x: u32) -> bool {
    x & 0x7FFF_FFFF == 0
}

fn zero(s: bool) -> u32 {
    u32::from(s) << 31
}

fn inf(s: bool) -> u32 {
    zero(s) | 0x7F80_0000
}

/// Flush a denormal input to zero if FZ (`FPUnpack`), raising IDC.
fn flush_in(x: u32, env: &mut Env) -> u32 {
    if env.fz && exp_field(x) == 0 && frac(x) != 0 {
        env.flags |= fpscr::IDC;
        x & 0x8000_0000
    } else {
        x
    }
}

/// `FPProcessNaN`: quiet a signalling NaN (raising IOC), or the default
/// NaN in DN mode.
fn process_nan(x: u32, env: &mut Env) -> u32 {
    if is_snan(x) {
        env.flags |= fpscr::IOC;
    }
    if env.dn {
        DEFAULT_NAN
    } else {
        x | QUIET
    }
}

/// `FPProcessNaNs` (and `FPProcessNaNs3`): the first signalling NaN, else
/// the first quiet NaN, of the operands in order.
fn process_nans(ops: &[u32], env: &mut Env) -> Option<u32> {
    if let Some(&x) = ops.iter().find(|&&x| is_snan(x)) {
        return Some(process_nan(x, env));
    }
    ops.iter()
        .find(|&&x| is_nan(x))
        .map(|&x| process_nan(x, env))
}

/// A finite nonzero value as (sign, exponent, significand): the value is
/// `significand * 2^exponent`, the significand an integer below 2^24.
fn unpack(x: u32) -> (bool, i32, u64) {
    let e = exp_field(x);
    let f = u64::from(frac(x));
    if e == 0 {
        (sign(x), -149, f)
    } else {
        (sign(x), e as i32 - 150, f | 0x0080_0000)
    }
}

/// Round `sig * 2^exp` (exact, apart from `sticky`: nonzero bits below
/// `sig`) to single precision. `sig` must be nonzero.
fn round_pack(s: bool, exp: i32, sig: u128, sticky: bool, env: &mut Env) -> u32 {
    // Normalise: the leading one at bit 127.
    let lz = sig.leading_zeros() as i32;
    let sig = sig << lz;
    // The value is 1.f * 2^e, e = exp + 127 - lz.
    let e = exp + 127 - lz;
    let biased = e + 127; // the exponent field it would have
    let tiny = biased < 1; // tininess before rounding
    if tiny && env.fz {
        env.flags |= fpscr::UFC;
        return zero(s);
    }
    // Bits of `sig` below the result's last significand bit.
    let shift: i32 = if tiny { 104 + (1 - biased) } else { 104 };
    let (mut mant, rem_nonzero, half, above_half) = if shift >= 129 {
        (0u128, true, false, false)
    } else if shift == 128 {
        let half = sig == 1u128 << 127 && !sticky;
        (0u128, true, half, !half) // sig >= 2^127: at least half
    } else {
        let mask = (1u128 << shift) - 1;
        let rem = sig & mask;
        let halfway = 1u128 << (shift - 1);
        (
            sig >> shift,
            rem != 0 || sticky,
            rem == halfway && !sticky,
            rem > halfway || (rem == halfway && sticky),
        )
    };
    let inexact = rem_nonzero;
    let increment = match env.rounding {
        Rounding::Nearest => above_half || (half && mant & 1 == 1),
        Rounding::PlusInf => inexact && !s,
        Rounding::MinusInf => inexact && s,
        Rounding::Zero => false,
    };
    if increment {
        mant += 1;
    }
    let mut biased = if tiny { 0 } else { biased };
    if !tiny && mant == 1 << 24 {
        mant >>= 1;
        biased += 1;
    }
    if biased >= 255 {
        env.flags |= fpscr::OFC | fpscr::IXC;
        let to_inf = match env.rounding {
            Rounding::Nearest => true,
            Rounding::PlusInf => !s,
            Rounding::MinusInf => s,
            Rounding::Zero => false,
        };
        return if to_inf {
            inf(s)
        } else {
            zero(s) | 0x7F7F_FFFF
        };
    }
    if inexact {
        env.flags |= fpscr::IXC;
        if tiny {
            env.flags |= fpscr::UFC;
        }
    }
    let mant = mant as u32;
    if tiny {
        // A denormal; a carry into bit 23 makes the smallest normal.
        zero(s) | mant
    } else {
        zero(s) | (biased as u32) << 23 | (mant & 0x007F_FFFF)
    }
}

/// `sa * 2^ea + sb * 2^eb` (signs applied), exactly enough to round.
fn add_exact(sa: bool, ea: i32, ma: u128, sb: bool, eb: i32, mb: u128, env: &mut Env) -> u32 {
    // Align on the smaller exponent, but shift by at most ALIGN bits: the
    // operands have at most 48 bits (a product), so they still fit in 128
    // bits with two more, and what falls below that, more than 25 bits
    // under the larger operand's leading one, only matters as a sticky bit.
    const ALIGN: i32 = 76;
    let (sa, ea, ma, sb, eb, mb) = if ea >= eb {
        (sa, ea, ma, sb, eb, mb)
    } else {
        (sb, eb, mb, sa, ea, ma)
    };
    let d = (ea - eb).min(ALIGN) as u32;
    let base = ea - d as i32;
    let a = ma << d;
    let (b, sticky) = if ea - eb > ALIGN {
        let lost = ea - eb - ALIGN;
        if lost >= 128 {
            (0u128, mb != 0)
        } else {
            (mb >> lost, mb & ((1u128 << lost) - 1) != 0)
        }
    } else {
        (mb, false)
    };
    // With a sticky remainder of b, represent it by an extra low bit
    // (enough guard bits remain for rounding).
    let (a, b, base) = if sticky {
        (a << 2, (b << 2) | 1, base - 2)
    } else {
        (a, b, base)
    };
    if sa == sb {
        round_pack(sa, base, a + b, false, env)
    } else if a >= b {
        let r = a - b;
        if r == 0 {
            zero(env.rounding == Rounding::MinusInf)
        } else {
            round_pack(sa, base, r, false, env)
        }
    } else {
        round_pack(sb, base, b - a, false, env)
    }
}

/// `FPAdd`.
#[must_use]
pub fn add(a: u32, b: u32, env: &mut Env) -> u32 {
    let (a, b) = (flush_in(a, env), flush_in(b, env));
    if let Some(n) = process_nans(&[a, b], env) {
        return n;
    }
    match (is_inf(a), is_inf(b)) {
        (true, true) if sign(a) != sign(b) => {
            env.flags |= fpscr::IOC;
            return DEFAULT_NAN;
        }
        (true, _) => return a,
        (_, true) => return b,
        _ => {}
    }
    match (is_zero(a), is_zero(b)) {
        (true, true) => {
            return if sign(a) == sign(b) {
                a
            } else {
                zero(env.rounding == Rounding::MinusInf)
            }
        }
        (true, false) => return b,
        (false, true) => return a,
        _ => {}
    }
    let (sa, ea, ma) = unpack(a);
    let (sb, eb, mb) = unpack(b);
    add_exact(sa, ea, u128::from(ma), sb, eb, u128::from(mb), env)
}

/// `FPSub`.
#[must_use]
pub fn sub(a: u32, b: u32, env: &mut Env) -> u32 {
    let b = flush_in(b, env);
    if is_nan(b) {
        // NaN operands keep their own sign.
        let a = flush_in(a, env);
        return process_nans(&[a, b], env).unwrap_or(DEFAULT_NAN);
    }
    add(a, b ^ 0x8000_0000, env)
}

/// `FPMul`.
#[must_use]
pub fn mul(a: u32, b: u32, env: &mut Env) -> u32 {
    let (a, b) = (flush_in(a, env), flush_in(b, env));
    if let Some(n) = process_nans(&[a, b], env) {
        return n;
    }
    let s = sign(a) != sign(b);
    if (is_inf(a) && is_zero(b)) || (is_zero(a) && is_inf(b)) {
        env.flags |= fpscr::IOC;
        return DEFAULT_NAN;
    }
    if is_inf(a) || is_inf(b) {
        return inf(s);
    }
    if is_zero(a) || is_zero(b) {
        return zero(s);
    }
    let (_, ea, ma) = unpack(a);
    let (_, eb, mb) = unpack(b);
    round_pack(s, ea + eb, u128::from(ma * mb), false, env)
}

/// `FPMulAdd(addend, a, b)`: `addend + a * b` with one rounding (VFMA and
/// family; the caller negates operands first as each instruction says).
#[must_use]
pub fn mul_add(addend: u32, a: u32, b: u32, env: &mut Env) -> u32 {
    let (c, a, b) = (flush_in(addend, env), flush_in(a, env), flush_in(b, env));
    let inf_zero = (is_inf(a) && is_zero(b)) || (is_zero(a) && is_inf(b));
    if is_nan(c) && !is_snan(c) && inf_zero && !is_snan(a) && !is_snan(b) {
        env.flags |= fpscr::IOC;
        return DEFAULT_NAN;
    }
    if let Some(n) = process_nans(&[c, a, b], env) {
        return n;
    }
    let sp = sign(a) != sign(b);
    if inf_zero {
        env.flags |= fpscr::IOC;
        return DEFAULT_NAN;
    }
    let prod_inf = is_inf(a) || is_inf(b);
    if prod_inf && is_inf(c) && sign(c) != sp {
        env.flags |= fpscr::IOC;
        return DEFAULT_NAN;
    }
    if prod_inf {
        return inf(sp);
    }
    if is_inf(c) {
        return c;
    }
    let prod_zero = is_zero(a) || is_zero(b);
    if prod_zero && is_zero(c) {
        return if sp == sign(c) {
            c
        } else {
            zero(env.rounding == Rounding::MinusInf)
        };
    }
    if prod_zero {
        // c + (+-0) = c, rounded (c is already a single: exact).
        return c;
    }
    let (_, ea, ma) = unpack(a);
    let (_, eb, mb) = unpack(b);
    let prod = u128::from(ma * mb);
    if is_zero(c) {
        return round_pack(sp, ea + eb, prod, false, env);
    }
    let (sc, ec, mc) = unpack(c);
    add_exact(sp, ea + eb, prod, sc, ec, u128::from(mc), env)
}

/// `FPDiv`.
#[must_use]
pub fn div(a: u32, b: u32, env: &mut Env) -> u32 {
    let (a, b) = (flush_in(a, env), flush_in(b, env));
    if let Some(n) = process_nans(&[a, b], env) {
        return n;
    }
    let s = sign(a) != sign(b);
    if (is_inf(a) && is_inf(b)) || (is_zero(a) && is_zero(b)) {
        env.flags |= fpscr::IOC;
        return DEFAULT_NAN;
    }
    if is_inf(a) {
        return inf(s);
    }
    if is_inf(b) || is_zero(a) {
        return zero(s);
    }
    if is_zero(b) {
        env.flags |= fpscr::DZC;
        return inf(s);
    }
    let (_, ea, ma) = unpack(a);
    let (_, eb, mb) = unpack(b);
    let num = u128::from(ma) << 80;
    let q = num / u128::from(mb);
    let r = num % u128::from(mb);
    round_pack(s, ea - eb - 80, q, r != 0, env)
}

/// Integer square root of `n` and whether it was exact.
fn isqrt(n: u128) -> (u128, bool) {
    if n == 0 {
        return (0, true);
    }
    // Newton's method from an over-estimate.
    let mut x = 1u128 << (128 - n.leading_zeros()).div_ceil(2);
    loop {
        let y = x.midpoint(n / x);
        if y >= x {
            break;
        }
        x = y;
    }
    (x, x * x == n)
}

/// `FPSqrt`.
#[must_use]
pub fn sqrt(a: u32, env: &mut Env) -> u32 {
    let a = flush_in(a, env);
    if is_nan(a) {
        return process_nan(a, env);
    }
    if is_zero(a) {
        return a;
    }
    if sign(a) {
        env.flags |= fpscr::IOC;
        return DEFAULT_NAN;
    }
    if is_inf(a) {
        return a;
    }
    let (_, e, m) = unpack(a);
    // sqrt(m * 2^e) = sqrt(m * 2^(e - k)) * 2^(k/2) with e - k even and
    // enough bits in m * 2^k for a 26-bit root and more.
    let k = if e & 1 == 0 { 100 } else { 101 };
    let (root, exact) = isqrt(u128::from(m) << k);
    round_pack(false, (e - k) / 2, root, !exact, env)
}

/// The result of `FPCompare` as the NZCV flags (bits 31:28).
#[must_use]
pub fn compare(a: u32, b: u32, quiet_nan_exc: bool, env: &mut Env) -> u32 {
    let (a, b) = (flush_in(a, env), flush_in(b, env));
    if is_nan(a) || is_nan(b) {
        if quiet_nan_exc || is_snan(a) || is_snan(b) {
            env.flags |= fpscr::IOC;
        }
        return 0x3000_0000; // unordered: C, V
    }
    if (is_zero(a) && is_zero(b)) || a == b {
        return 0x6000_0000; // equal: Z, C
    }
    let less = match (sign(a), sign(b)) {
        (true, false) => true,
        (false, true) => false,
        (false, false) => a < b,
        (true, true) => a > b,
    };
    if less {
        0x8000_0000 // N
    } else {
        0x2000_0000 // C
    }
}

/// `FPToFixed`: `a * 2^frac_bits` as a `size`-bit integer (signed unless
/// `unsigned`), rounded as `rounding` says, saturating; returned as the
/// low `size` bits, sign- or zero-extended to 32.
#[must_use]
pub fn to_fixed(
    a: u32,
    size: u32,
    frac_bits: u32,
    unsigned: bool,
    rounding: Rounding,
    env: &mut Env,
) -> u32 {
    let a = flush_in(a, env);
    if is_nan(a) {
        env.flags |= fpscr::IOC;
        return 0;
    }
    let (min, max): (i128, i128) = if unsigned {
        (0, (1i128 << size) - 1)
    } else {
        (-(1i128 << (size - 1)), (1i128 << (size - 1)) - 1)
    };
    let extend = |v: i128| -> u32 {
        let bits = (v as u32)
            & if size == 32 {
                u32::MAX
            } else {
                (1 << size) - 1
            };
        if !unsigned && size < 32 && bits & (1 << (size - 1)) != 0 {
            bits | !((1 << size) - 1)
        } else {
            bits
        }
    };
    let saturate = |v: i128, env: &mut Env| -> u32 {
        env.flags |= fpscr::IOC;
        extend(v)
    };
    if is_inf(a) {
        return saturate(if sign(a) { min } else { max }, env);
    }
    if is_zero(a) {
        return 0;
    }
    let (s, e, m) = unpack(a);
    let e = e + frac_bits as i32;
    // The integer part and the fraction bits below it.
    let (int, rem_nonzero, half, above_half) = if e >= 0 {
        if e > 64 {
            return saturate(if s { min } else { max }, env);
        }
        (u128::from(m) << e, false, false, false)
    } else {
        let shift = (-e) as u32;
        if shift > 100 {
            (0u128, true, false, false)
        } else {
            let mm = u128::from(m);
            let rem = mm & ((1u128 << shift) - 1);
            let halfway = 1u128 << (shift - 1);
            (mm >> shift, rem != 0, rem == halfway, rem > halfway)
        }
    };
    let increment = match rounding {
        Rounding::Nearest => above_half || (half && int & 1 == 1),
        Rounding::PlusInf => rem_nonzero && !s,
        Rounding::MinusInf => rem_nonzero && s,
        Rounding::Zero => false,
    };
    let magnitude = int as i128 + i128::from(increment);
    let v = if s { -magnitude } else { magnitude };
    if v < min || v > max {
        return saturate(if v < min { min } else { max }, env);
    }
    if rem_nonzero {
        env.flags |= fpscr::IXC;
    }
    extend(v)
}

/// `FixedToFP`: the `size`-bit integer `v` (signed unless `unsigned`) times
/// `2^-frac_bits`, rounded as `rounding` says.
#[must_use]
pub fn from_fixed(
    v: u32,
    size: u32,
    frac_bits: u32,
    unsigned: bool,
    rounding: Rounding,
    env: &mut Env,
) -> u32 {
    let v = if size == 32 { v } else { v & ((1 << size) - 1) };
    let value: i64 = if unsigned {
        i64::from(v)
    } else if size == 32 {
        i64::from(v as i32)
    } else {
        i64::from(((v << (32 - size)) as i32) >> (32 - size))
    };
    if value == 0 {
        return 0;
    }
    let saved = env.rounding;
    env.rounding = rounding;
    let r = round_pack(
        value < 0,
        -(frac_bits as i32),
        u128::from(value.unsigned_abs()),
        false,
        env,
    );
    env.rounding = saved;
    r
}

/// `FPHalfToSingle` (VCVTB/VCVTT to single). FZ does not apply to
/// half-precision values.
#[must_use]
pub fn half_to_single(h: u16, env: &mut Env) -> u32 {
    let s = u32::from(h >> 15) << 31;
    let e = u32::from((h >> 10) & 0x1F);
    let f = u32::from(h & 0x3FF);
    if e == 0x1F && !env.ahp {
        if f == 0 {
            return s | 0x7F80_0000;
        }
        let nan = s | 0x7F80_0000 | (f << 13);
        return process_nan(nan, env);
    }
    if e == 0 {
        if f == 0 {
            return s;
        }
        // A half-precision denormal is a normal single.
        let lz = f.leading_zeros() - 22; // zeros within the 10-bit field
        let f = (f << (lz + 1)) & 0x3FF;
        return s | (112 - lz) << 23 | (f << 13);
    }
    s | (e + 112) << 23 | (f << 13)
}

/// `FPSingleToHalf` (VCVTB/VCVTT to half), with FPSCR rounding.
#[must_use]
pub fn single_to_half(a: u32, env: &mut Env) -> u16 {
    let a = flush_in(a, env);
    let s = (u16::from(sign(a))) << 15;
    if is_nan(a) {
        if env.ahp {
            env.flags |= fpscr::IOC;
            return s;
        }
        if is_snan(a) {
            env.flags |= fpscr::IOC;
        }
        if env.dn {
            return 0x7E00;
        }
        return s | 0x7E00 | ((frac(a) >> 13) as u16 & 0x3FF);
    }
    if is_inf(a) {
        if env.ahp {
            env.flags |= fpscr::IOC;
            return s | 0x7FFF;
        }
        return s | 0x7C00;
    }
    if is_zero(a) {
        return s;
    }
    let (sg, e, m) = unpack(a);
    // Round m * 2^e to 11 significant bits, half-precision range.
    let lz = m.leading_zeros() as i32 - 40; // m < 2^24: normalise to bit 23
    let m = m << lz;
    let e = e - lz; // value = m * 2^e, m in [2^23, 2^24)
    let unbiased = e + 23; // value = 1.f * 2^unbiased
    let biased = unbiased + 15;
    let tiny = biased < 1;
    let shift: i32 = if tiny { 13 + (1 - biased) } else { 13 };
    let (mut mant, inexact, half, above) = if shift >= 64 {
        (0u64, true, false, false)
    } else {
        let rem = m & ((1u64 << shift) - 1);
        let halfway = 1u64 << (shift - 1);
        (m >> shift, rem != 0, rem == halfway, rem > halfway)
    };
    let inc = match env.rounding {
        Rounding::Nearest => above || (half && mant & 1 == 1),
        Rounding::PlusInf => inexact && !sg,
        Rounding::MinusInf => inexact && sg,
        Rounding::Zero => false,
    };
    mant += u64::from(inc);
    let mut biased = if tiny { 0 } else { biased };
    if !tiny && mant == 1 << 11 {
        mant >>= 1;
        biased += 1;
    }
    let max_biased = if env.ahp { 32 } else { 31 };
    if biased >= max_biased {
        if env.ahp {
            env.flags |= fpscr::IOC;
            return s | 0x7FFF;
        }
        env.flags |= fpscr::OFC | fpscr::IXC;
        let to_inf = match env.rounding {
            Rounding::Nearest => true,
            Rounding::PlusInf => !sg,
            Rounding::MinusInf => sg,
            Rounding::Zero => false,
        };
        return if to_inf { s | 0x7C00 } else { s | 0x7BFF };
    }
    if inexact {
        env.flags |= fpscr::IXC;
        if tiny {
            env.flags |= fpscr::UFC;
        }
    }
    let mant = mant as u16;
    if tiny {
        s | mant
    } else {
        s | (biased as u16) << 10 | (mant & 0x3FF)
    }
}

/// `VFPExpandImm` for single precision (VMOV immediate).
#[must_use]
pub fn expand_imm(imm8: u32) -> u32 {
    let s = (imm8 >> 7) & 1;
    let b6 = (imm8 >> 6) & 1;
    let exp = ((b6 ^ 1) << 7) | (if b6 == 1 { 0x1F } else { 0 } << 2) | ((imm8 >> 4) & 3);
    s << 31 | exp << 23 | (imm8 & 0xF) << 19
}

#[cfg(test)]
#[allow(clippy::cast_precision_loss)] // the host's conversions are the reference
mod tests {
    use super::*;

    fn env() -> Env {
        Env::from_fpscr(0)
    }

    fn f(x: f32) -> u32 {
        x.to_bits()
    }

    #[test]
    fn simple_arithmetic_matches_the_host_in_round_to_nearest() {
        let values = [
            0.0f32, -0.0, 1.0, -1.0, 1.5, 0.1, 3.0, 1e-38, 1e-40, 3.4e38, -2.5e-42, 7.0, 1e10,
            123.456,
        ];
        for &a in &values {
            for &b in &values {
                let mut e = env();
                assert_eq!(add(f(a), f(b), &mut e), f(a + b), "{a} + {b}");
                assert_eq!(sub(f(a), f(b), &mut e), f(a - b), "{a} - {b}");
                assert_eq!(mul(f(a), f(b), &mut e), f(a * b), "{a} * {b}");
                if b != 0.0 {
                    assert_eq!(div(f(a), f(b), &mut e), f(a / b), "{a} / {b}");
                }
                assert_eq!(
                    mul_add(f(a), f(b), f(3.0), &mut e),
                    f(b.mul_add(3.0, a)),
                    "fma {a} {b}"
                );
            }
            if a >= 0.0 {
                assert_eq!(sqrt(f(a), &mut env()), f(a.sqrt()), "sqrt {a}");
            }
        }
    }

    /// Random operands biased towards the edges (denormals, huge values,
    /// values close to each other) against the host's IEEE arithmetic,
    /// which rounds to nearest. NaN results are compared as NaN only (the
    /// host's NaN propagation differs from Arm's; QEMU checks those).
    #[test]
    fn random_operands_match_the_host_in_round_to_nearest() {
        let mut seed = 0x9E37_79B9_7F4A_7C15u64;
        let mut next = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        let pick = |n: u64| -> u32 {
            let r = n as u32;
            match (n >> 32) % 6 {
                0 => r & 0x807F_FFFF,                 // denormal or zero
                1 => (r & 0x80FF_FFFF) | 0x7E00_0000, // huge
                2 => (r & 0x80FF_FFFF) | 0x0080_0000, // tiny normal
                3 => (r & 0x81FF_FFFF) | 0x3F00_0000, // near 1
                _ => r,
            }
        };
        let same = |x: u32, y: u32| x == y || (is_nan(x) && is_nan(y));
        for _ in 0..300_000 {
            let (x, y, z) = (pick(next()), pick(next()), pick(next()));
            let (a, b, c) = (f32::from_bits(x), f32::from_bits(y), f32::from_bits(z));
            let mut e = env();
            assert!(
                same(add(x, y, &mut e), (a + b).to_bits()),
                "{x:#x} + {y:#x}"
            );
            assert!(
                same(sub(x, y, &mut e), (a - b).to_bits()),
                "{x:#x} - {y:#x}"
            );
            assert!(
                same(mul(x, y, &mut e), (a * b).to_bits()),
                "{x:#x} * {y:#x}"
            );
            assert!(
                same(div(x, y, &mut e), (a / b).to_bits()),
                "{x:#x} / {y:#x}"
            );
            assert!(same(sqrt(x, &mut e), a.sqrt().to_bits()), "sqrt {x:#x}");
            assert!(
                same(mul_add(z, x, y, &mut e), a.mul_add(b, c).to_bits()),
                "{z:#x} + {x:#x} * {y:#x}"
            );
            assert!(
                same(
                    from_fixed(x, 32, 0, false, Rounding::Nearest, &mut e),
                    (x as i32 as f32).to_bits()
                ),
                "s32 {x:#x}"
            );
            assert!(
                same(
                    from_fixed(x, 32, 0, true, Rounding::Nearest, &mut e),
                    (x as f32).to_bits()
                ),
                "u32 {x:#x}"
            );
            if !a.is_nan() {
                // Rust's `as` truncates and saturates, as VCVT (round to zero) does.
                assert_eq!(
                    to_fixed(x, 32, 0, false, Rounding::Zero, &mut e),
                    a as i32 as u32,
                    "to s32 {x:#x}"
                );
                assert_eq!(
                    to_fixed(x, 32, 0, true, Rounding::Zero, &mut e),
                    a as u32,
                    "to u32 {x:#x}"
                );
            }
        }
    }

    #[test]
    fn flags_and_special_cases() {
        let mut e = env();
        assert_eq!(div(f(1.0), f(0.0), &mut e), f(f32::INFINITY));
        assert_eq!(e.flags, fpscr::DZC);
        let mut e = env();
        assert_eq!(
            add(f(f32::INFINITY), f(f32::NEG_INFINITY), &mut e),
            DEFAULT_NAN
        );
        assert_eq!(e.flags, fpscr::IOC);
        let mut e = env();
        assert_eq!(mul(f(3.4e38), f(10.0), &mut e), f(f32::INFINITY));
        assert_eq!(e.flags, fpscr::OFC | fpscr::IXC);
        let mut e = env();
        let _ = div(f(1.0), f(3.0), &mut e);
        assert_eq!(e.flags, fpscr::IXC);
        // Signalling NaN: quieted, IOC.
        let mut e = env();
        assert_eq!(add(0x7F80_0001, f(1.0), &mut e), 0x7FC0_0001);
        assert_eq!(e.flags, fpscr::IOC);
        // Default NaN mode.
        let mut e = Env::from_fpscr(fpscr::DN);
        assert_eq!(add(0x7FC0_1234, f(1.0), &mut e), DEFAULT_NAN);
        // Flush to zero: denormal input -> IDC; tiny result -> UFC.
        let mut e = Env::from_fpscr(fpscr::FZ);
        assert_eq!(add(1, f(0.0), &mut e), 0);
        assert_eq!(e.flags, fpscr::IDC);
        let mut e = Env::from_fpscr(fpscr::FZ);
        assert_eq!(mul(f(1e-30), f(1e-10), &mut e), 0);
        assert_eq!(e.flags, fpscr::UFC);
        // Underflow without FZ: tiny and inexact.
        let mut e = env();
        let _ = mul(f(1e-30), f(1.1e-10), &mut e);
        assert_eq!(e.flags, fpscr::UFC | fpscr::IXC);
        // Rounding modes.
        let third = |mode: u32| {
            let mut e = Env::from_fpscr(mode << fpscr::RMODE_SHIFT);
            div(f(1.0), f(3.0), &mut e)
        };
        // 1/3 = 0x3EAAAAAA.AA..: nearest rounds up.
        assert_eq!(third(0), 0x3EAA_AAAB);
        assert_eq!(third(1), 0x3EAA_AAAB, "towards +inf");
        assert_eq!(third(2), 0x3EAA_AAAA, "towards -inf");
        assert_eq!(third(3), 0x3EAA_AAAA, "towards zero");
        let mut e = Env::from_fpscr(2 << fpscr::RMODE_SHIFT);
        assert_eq!(
            sub(f(1.0), f(1.0), &mut e),
            f(-0.0),
            "x - x is -0 rounding down"
        );
    }

    #[test]
    fn compare_and_conversions() {
        let mut e = env();
        assert_eq!(compare(f(1.0), f(2.0), false, &mut e), 0x8000_0000);
        assert_eq!(compare(f(2.0), f(1.0), false, &mut e), 0x2000_0000);
        assert_eq!(compare(f(0.0), f(-0.0), false, &mut e), 0x6000_0000);
        assert_eq!(compare(DEFAULT_NAN, f(1.0), false, &mut e), 0x3000_0000);
        assert_eq!(e.flags, 0);
        let _ = compare(DEFAULT_NAN, f(1.0), true, &mut e);
        assert_eq!(e.flags, fpscr::IOC);
        let mut e = env();
        assert_eq!(
            to_fixed(f(-2.7), 32, 0, false, Rounding::Zero, &mut e),
            (-2i32) as u32
        );
        assert_eq!(e.flags, fpscr::IXC);
        assert_eq!(
            to_fixed(f(2.5), 32, 0, false, Rounding::Nearest, &mut env()),
            2
        );
        assert_eq!(
            to_fixed(f(3.5), 32, 0, false, Rounding::Nearest, &mut env()),
            4
        );
        let mut e = env();
        assert_eq!(
            to_fixed(f(1e20), 32, 0, false, Rounding::Zero, &mut e),
            i32::MAX as u32
        );
        assert_eq!(e.flags, fpscr::IOC);
        assert_eq!(
            to_fixed(f(-1.0), 32, 0, true, Rounding::Zero, &mut env()),
            0
        );
        assert_eq!(
            to_fixed(f(1.5), 16, 4, false, Rounding::Zero, &mut env()),
            24
        );
        assert_eq!(
            to_fixed(f(-1.5), 16, 4, false, Rounding::Zero, &mut env()),
            (-24i32) as u32
        );
        assert_eq!(
            from_fixed(24, 16, 4, false, Rounding::Nearest, &mut env()),
            f(1.5)
        );
        assert_eq!(
            from_fixed(0xFFE8, 16, 4, false, Rounding::Nearest, &mut env()),
            f(-1.5)
        );
        assert_eq!(
            from_fixed(16_777_217, 32, 0, false, Rounding::Nearest, &mut env()),
            f(16_777_216.0)
        );
        assert_eq!(
            from_fixed(u32::MAX, 32, 0, true, Rounding::Nearest, &mut env()),
            f(4_294_967_296.0)
        );
        // Half precision.
        assert_eq!(single_to_half(f(1.0), &mut env()), 0x3C00);
        assert_eq!(
            single_to_half(f(65520.0), &mut env()),
            0x7C00,
            "rounds to infinity"
        );
        assert_eq!(
            single_to_half(f(5.960_464_5e-8), &mut env()),
            0x0001,
            "smallest denormal"
        );
        assert_eq!(half_to_single(0x3C00, &mut env()), f(1.0));
        assert_eq!(half_to_single(0x0001, &mut env()), f(5.960_464_5e-8));
        assert_eq!(half_to_single(0xFC00, &mut env()), f(f32::NEG_INFINITY));
        // VMOV immediate.
        assert_eq!(expand_imm(0x70), f(1.0));
        assert_eq!(expand_imm(0x78), f(1.5));
        assert_eq!(expand_imm(0xD0), f(-0.25));
    }
}
