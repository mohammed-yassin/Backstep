//! Backstep's replay engine.
//!
//! A recording (`docs/adr/0008-recording-format.md`) holds the starting
//! state of a stretch of execution, every peripheral load in it, and the
//! state at its end. Replay loads the firmware ELF and the starting state
//! into the emulator (`backstep-emu`), answers each peripheral load from
//! the recording while checking it is the same load (address, size and
//! instruction), and at the end checks the registers and a checksum of the
//! RAM. Any difference is reported as a divergence, not ignored.
//!
//! Interrupts are delivered where the recording says they arrived
//! (`docs/adr/0009-replaying-interrupts.md`). [`Timeline`] moves through
//! a checked recording forwards and backwards (reverse execution).

// Register fields are narrowed to their architectural widths on purpose,
// and the documentation uses the manual's register names (SysTick, DWT).
#![allow(clippy::cast_possible_truncation, clippy::doc_markdown)]

pub mod recording;
pub mod replay;
pub mod target;
pub mod timeline;

pub use recording::{parse, FormatError, Recording};
pub use replay::{replay, Options, ReplayError, Report};
pub use timeline::{Stop, Timeline, Watchpoint};
