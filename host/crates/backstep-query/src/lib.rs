//! Questions about a replayed recording, answered from its exact history:
//!
//! - [`why`]: who wrote a variable last (instruction, source line,
//!   function, interrupt or main code), with its value before and after,
//!   every earlier write, and **lost updates**: writes based on a value
//!   that another context changed in the meantime (a race between an
//!   interrupt and the code it interrupted).
//! - [`events`]: the interrupt and fault timeline.
//!
//! Both work on a [`Timeline`] (the recording, checked) and the firmware's
//! [`DebugInfo`]. See `docs/adr/0011-queries.md`.

// Target addresses and sizes are 32-bit: DWARF values are narrowed to
// them on purpose.
#![allow(clippy::doc_markdown, clippy::cast_possible_truncation)]

pub mod debug;

use backstep_emu::memory::WatchKind;
use backstep_emu::Cpu;
use backstep_replay::{ReplayError, Stop, Timeline, Watchpoint};
use serde::Serialize;

pub use debug::{DebugInfo, Location, LookupError, Variable};

/// The context an instruction ran in: the main code (thread mode) or an
/// exception handler.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Context {
    /// Thread mode: the main code (or an RTOS task).
    Thread,
    /// An exception handler, by exception number.
    Handler {
        /// Exception number (IPSR).
        exception: u32,
        /// Its name.
        name: String,
    },
}

impl Context {
    fn of(cpu: &Cpu) -> Context {
        match cpu.ipsr {
            0 => Context::Thread,
            n => Context::Handler {
                exception: n,
                name: exception_name(n),
            },
        }
    }
}

impl std::fmt::Display for Context {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Context::Thread => f.write_str("main code (thread mode)"),
            Context::Handler { name, .. } => write!(f, "{name} handler"),
        }
    }
}

/// The architectural name of exception `n` (`IRQ<n-16>` for interrupts).
#[must_use]
pub fn exception_name(n: u32) -> String {
    match n {
        1 => "Reset".into(),
        2 => "NMI".into(),
        3 => "HardFault".into(),
        4 => "MemManage".into(),
        5 => "BusFault".into(),
        6 => "UsageFault".into(),
        11 => "SVCall".into(),
        12 => "DebugMonitor".into(),
        14 => "PendSV".into(),
        15 => "SysTick".into(),
        n if n >= 16 => format!("IRQ{}", n - 16),
        n => format!("exception {n}"),
    }
}

/// One access to a variable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Access {
    /// Position of the instruction (instructions since the start).
    pub position: u64,
    /// Read or write.
    pub write: bool,
    /// Address and size of the access.
    pub address: u32,
    /// Bytes.
    pub size: u8,
    /// The instruction.
    pub at: Location,
    /// What it ran in.
    pub context: Context,
    /// The stack pointer when it ran.
    pub sp: u32,
    /// The access was to the stack pointer's neighbourhood (within 64
    /// bytes): for a global variable, the stack has grown into it.
    pub near_stack_pointer: bool,
    /// The variable's value before and after (little-endian, at most 8
    /// bytes; `None` for larger variables).
    pub before: Option<u64>,
    /// See `before`.
    pub after: Option<u64>,
}

/// A write based on a stale read: between the read and the write,
/// another context wrote the variable, and that write was overwritten.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LostUpdate {
    /// The read the write was based on.
    pub read: Access,
    /// The writes of other contexts after it, lost.
    pub lost: Vec<Access>,
    /// The write that overwrote them.
    pub write: Access,
}

/// The answer to "who wrote this?".
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Why {
    /// The variable.
    pub variable: Variable,
    /// The position asked about.
    pub position: u64,
    /// Its value there (little-endian, at most 8 bytes).
    pub value: Option<u64>,
    /// The last write before that position, if any.
    pub last_write: Option<Access>,
    /// All writes up to the position, oldest first.
    pub writes: Vec<Access>,
    /// Reads up to the position.
    pub reads: usize,
    /// Writes that overwrote another context's write they had not read.
    pub lost_updates: Vec<LostUpdate>,
}

fn value_of(t: &Timeline<'_>, v: &Variable) -> Option<u64> {
    if v.size > 8 {
        return None;
    }
    let bytes = t.read_memory(v.address, v.size);
    bytes
        .iter()
        .rev()
        .try_fold(0u64, |acc, b| b.map(|b| (acc << 8) | u64::from(b)))
}

/// Every access to `v` from the start to position `upto`, by replaying
/// with an access watchpoint on it.
///
/// # Errors
/// [`ReplayError`] if the replay fails (a checked recording does not).
pub fn accesses(
    t: &mut Timeline<'_>,
    debug: &DebugInfo<'_>,
    v: &Variable,
    upto: u64,
) -> Result<Vec<Access>, ReplayError> {
    let w = Watchpoint {
        address: v.address,
        len: v.size,
        kind: WatchKind::Access,
    };
    t.seek(0)?;
    t.add_watchpoint(w);
    let mut out = Vec::new();
    let mut value = value_of(t, v);
    while t.position() < upto {
        let pc = t.cpu().r[15];
        let sp = t.cpu().r[13];
        let context = Context::of(t.cpu());
        let position = t.position();
        match t.step()? {
            Stop::Watch(hit) => {
                let after = value_of(t, v);
                out.push(Access {
                    position,
                    write: hit.write,
                    address: hit.address,
                    size: hit.size,
                    at: debug.location(pc),
                    context: context.clone(),
                    sp,
                    near_stack_pointer: hit.address.wrapping_sub(sp).wrapping_add(64) < 128,
                    before: value,
                    after,
                });
                value = after;
            }
            Stop::End => break,
            _ => value = value_of(t, v),
        }
    }
    t.remove_watchpoint(w);
    Ok(out)
}

/// Who wrote `v` last before position `at` (the end of the recording if
/// `None`), every write before that, and the lost updates among them.
///
/// # Errors
/// [`ReplayError`] if the replay fails (a checked recording does not).
pub fn why(
    t: &mut Timeline<'_>,
    debug: &DebugInfo<'_>,
    v: &Variable,
    at: Option<u64>,
) -> Result<Why, ReplayError> {
    let position = at.unwrap_or(t.end()).min(t.end());
    let all = accesses(t, debug, v, position)?;
    t.seek(position)?;
    let value = value_of(t, v);
    let writes: Vec<Access> = all.iter().filter(|a| a.write).cloned().collect();
    let reads = all.len() - writes.len();
    Ok(Why {
        variable: v.clone(),
        position,
        value,
        last_write: writes.last().cloned(),
        lost_updates: lost_updates(&all),
        writes,
        reads,
    })
}

/// Writes based on a stale value: a context read the variable, another
/// context then wrote it, and the first context wrote it without reading
/// it again.
fn lost_updates(all: &[Access]) -> Vec<LostUpdate> {
    let mut out = Vec::new();
    for (i, w) in all.iter().enumerate() {
        if !w.write {
            continue;
        }
        // The same context's last read of it before this write, with no
        // write of its own in between.
        let Some(r) = all[..i]
            .iter()
            .rposition(|a| a.context == w.context)
            .filter(|&j| !all[j].write)
        else {
            continue;
        };
        let lost: Vec<Access> = all[r + 1..i]
            .iter()
            .filter(|a| a.write && a.context != w.context)
            .cloned()
            .collect();
        if !lost.is_empty() {
            out.push(LostUpdate {
                read: all[r].clone(),
                lost,
                write: w.clone(),
            });
        }
    }
    out
}

/// Something that happened in the recording.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum EventKind {
    /// The start of the recording.
    Start,
    /// An exception was entered: a recorded interrupt, a fault, SVC or
    /// PendSV.
    Entry {
        /// Exception number.
        exception: u32,
        /// Its name.
        name: String,
        /// A fault (HardFault, MemManage, BusFault, UsageFault).
        fault: bool,
        /// Fault causes the core recorded (CFSR bits, by name).
        causes: Vec<String>,
        /// Entered straight from another handler's return (tail-chained).
        tail_chained: bool,
        /// It preempted another handler.
        nested: bool,
    },
    /// A handler returned (to another handler or to the main code).
    Return {
        /// Exception number of the handler that returned.
        exception: u32,
        /// Its name.
        name: String,
    },
    /// The end of the recording.
    End,
}

/// An event and where it happened.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Event {
    /// Position.
    pub position: u64,
    /// What.
    #[serde(flatten)]
    pub kind: EventKind,
    /// For an entry: the instruction that was interrupted or faulted (the
    /// stacked return address). Otherwise where the core is.
    pub at: Location,
    /// For a fault: the instruction executed before it (a bad branch, for
    /// a fault at its target).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous: Option<Location>,
}

/// The interrupt and fault timeline of the whole recording.
///
/// # Errors
/// [`ReplayError`] if the replay fails (a checked recording does not).
pub fn events(t: &mut Timeline<'_>, debug: &DebugInfo<'_>) -> Result<Vec<Event>, ReplayError> {
    t.seek(0)?;
    let mut out = vec![Event {
        position: 0,
        kind: EventKind::Start,
        at: debug.location(t.cpu().r[15]),
        previous: None,
    }];
    let mut previous_pc = None;
    let counts = |cpu: &Cpu| -> (u64, u64, u64, u64, [u64; 6]) {
        let s = &cpu.stats;
        (
            s.entries.iter().sum(),
            s.returns.iter().sum(),
            s.tail_chained,
            s.nested,
            s.faults,
        )
    };
    let mut before = counts(t.cpu());
    loop {
        let ipsr = t.cpu().ipsr;
        let pc = t.cpu().r[15];
        let before_pc = previous_pc.replace(pc);
        if t.step()? == Stop::End {
            break;
        }
        let cpu = t.cpu();
        if cpu.ipsr == ipsr {
            continue;
        }
        let now = counts(cpu);
        let position = t.position();
        if now.1 > before.1 {
            out.push(Event {
                position,
                kind: EventKind::Return {
                    exception: ipsr,
                    name: exception_name(ipsr),
                },
                at: debug.location(cpu.r[15]),
                previous: None,
            });
        }
        if now.0 > before.0 {
            let n = cpu.ipsr;
            let causes: Vec<String> = backstep_emu::cpu::exceptions::FAULT_CAUSES
                .iter()
                .zip(now.4.iter().zip(before.4.iter()))
                .filter(|(_, (a, b))| a > b)
                .map(|((name, _), _)| (*name).to_string())
                .collect();
            // The stacked return address: the interrupted (or faulting)
            // instruction; for a tail-chain, the one the first handler
            // interrupted.
            let stacked = cpu.memory.peek(cpu.r[13].wrapping_add(24), 4).unwrap_or(pc);
            let fault = (3..=6).contains(&n);
            out.push(Event {
                position,
                kind: EventKind::Entry {
                    exception: n,
                    name: exception_name(n),
                    fault,
                    causes,
                    tail_chained: now.2 > before.2,
                    nested: now.3 > before.3,
                },
                at: debug.location(stacked),
                previous: if fault {
                    before_pc.map(|p| debug.location(p))
                } else {
                    None
                },
            });
        }
        before = now;
    }
    out.push(Event {
        position: t.position(),
        kind: EventKind::End,
        at: debug.location(t.cpu().r[15]),
        previous: None,
    });
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn access(position: u64, write: bool, ctx: u32, after: u64) -> Access {
        Access {
            position,
            write,
            address: 0x2000_0000,
            size: 4,
            at: Location {
                address: 0x100 + ctx,
                function: None,
                file: None,
                line: None,
            },
            context: if ctx == 0 {
                Context::Thread
            } else {
                Context::Handler {
                    exception: ctx,
                    name: exception_name(ctx),
                }
            },
            sp: 0,
            near_stack_pointer: false,
            before: None,
            after: Some(after),
        }
    }

    #[test]
    fn a_write_based_on_a_stale_read_is_a_lost_update() {
        // main reads 5; the handler writes 6; main writes 6 (5 + 1).
        let all = [
            access(1, false, 0, 5),
            access(2, false, 24, 5),
            access(3, true, 24, 6),
            access(4, true, 0, 6),
        ];
        let lost = lost_updates(&all);
        assert_eq!(lost.len(), 1);
        assert_eq!(lost[0].read.position, 1);
        assert_eq!(lost[0].lost.len(), 1);
        assert_eq!(lost[0].lost[0].position, 3);
        assert_eq!(lost[0].write.position, 4);
    }

    #[test]
    fn rereading_or_writing_first_is_not_a_lost_update() {
        // The handler's write is read again before main writes.
        let reread = [
            access(1, false, 0, 5),
            access(2, true, 24, 6),
            access(3, false, 0, 6),
            access(4, true, 0, 7),
        ];
        assert_eq!(lost_updates(&reread).len(), 0);
        // Main writes without reading first (a plain store, not an update).
        let blind = [access(1, true, 24, 6), access(2, true, 0, 0)];
        assert_eq!(lost_updates(&blind).len(), 0);
        // Same context only.
        let alone = [
            access(1, false, 0, 5),
            access(2, true, 0, 6),
            access(3, true, 0, 7),
        ];
        assert_eq!(lost_updates(&alone).len(), 0);
    }

    #[test]
    fn exception_names() {
        assert_eq!(exception_name(3), "HardFault");
        assert_eq!(exception_name(15), "SysTick");
        assert_eq!(exception_name(24), "IRQ8");
        assert_eq!(exception_name(7), "exception 7");
        assert_eq!(Context::Thread.to_string(), "main code (thread mode)");
    }
}
