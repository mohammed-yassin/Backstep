//! Cortex-M4 (ARMv7E-M) instruction-set emulator for Backstep.
//!
//! The replay engine runs the user's firmware binary on this emulator and
//! feeds it the values recorded on the real chip. It is written from
//! scratch so that Backstep stays MIT licensed (ADR 0001), and it is
//! verified instruction by instruction against QEMU (`docs/TESTING.md`).
//!
//! Peripherals are never modelled: every access outside RAM and flash goes
//! to a [`Bus`] implementation, which during replay answers loads from the
//! recording.
//!
//! Status (milestone M2, part 2): every 16-bit Thumb and 32-bit Thumb-2
//! integer and DSP instruction of ARMv7E-M, IT blocks, the special
//! registers (MSR/MRS, PRIMASK, BASEPRI, FAULTMASK, CONTROL, MSP/PSP),
//! exclusives, and the ELF loader. Exceptions follow in part 3 of M2 and
//! floating point in M7.

// An instruction-set emulator converts between register widths and
// signedness constantly, always deliberately; these lints would add noise
// to nearly every line without catching anything.
// Register names (rd, rn, rm) and encoding fields (h, imm) mirror the
// Architecture Reference Manual on purpose, and instruction groups are
// kept together in one function each, as the manual's decode tables are.
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap,
    clippy::cast_lossless,
    clippy::many_single_char_names,
    clippy::similar_names,
    clippy::too_many_lines,
    clippy::verbose_bit_mask
)]

pub mod alu;
pub mod cpu;
pub mod elf;
pub mod memory;

pub use cpu::{Cpu, Stop};
pub use memory::{Bus, Fault, Memory};
