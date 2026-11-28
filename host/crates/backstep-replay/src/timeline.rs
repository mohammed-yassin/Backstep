//! Moving through a replayed recording, forwards and backwards: the
//! engine behind `backstep gdb` (reverse execution).
//!
//! A position is the number of instructions executed since the start of
//! the recording; the state at a position is the machine before that
//! instruction, with the interrupts due there already delivered (as in a
//! `--trace` file). [`Timeline::new`] replays and checks the recording
//! once, then replays it again keeping a checkpoint (a copy-on-write
//! clone of the machine) every [`Timeline::interval`] instructions. Going
//! to any position restores the checkpoint before it and runs forward, so
//! a step back costs at most one interval of instructions.

use std::collections::BTreeSet;

use backstep_emu::elf::Elf;
use backstep_emu::memory::{WatchHit, WatchKind};
use backstep_emu::Cpu;

use crate::recording::Recording;
use crate::replay::{Engine, Machine, Plan, ReplayError};

/// Why a movement stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stop {
    /// One instruction was stepped (forwards or backwards).
    Step,
    /// A breakpoint address was reached.
    Breakpoint,
    /// A watched access: forwards, the instruction that made it has run;
    /// backwards, the timeline stops before it.
    Watch(WatchHit),
    /// The end of the recording: nothing more to run forwards.
    End,
    /// The start of the recording: nothing more to run backwards.
    Start,
    /// Asked to stop (the debugger's interrupt).
    Interrupted,
}

/// A watched address range.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Watchpoint {
    /// First byte.
    pub address: u32,
    /// Bytes.
    pub len: u32,
    /// Accesses that stop.
    pub kind: WatchKind,
}

/// How often a movement asks whether to stop (instructions).
const POLL_EVERY: u64 = 1 << 16;
/// Most checkpoints kept; the interval grows with the recording.
const MAX_CHECKPOINTS: u64 = 2048;
/// Smallest interval between checkpoints.
const MIN_INTERVAL: u64 = 4096;

/// A recording, replayed and checked, that can be moved through.
pub struct Timeline<'r> {
    engine: Engine<'r>,
    plan: Plan,
    checkpoints: Vec<Machine>,
    interval: u64,
    end: u64,
    now: Machine,
    breakpoints: BTreeSet<u32>,
    watchpoints: Vec<Watchpoint>,
}

impl<'r> Timeline<'r> {
    /// Replay and check `rec` on `elf` (as `backstep replay` does), then
    /// prepare the checkpoints. The timeline starts at position 0.
    ///
    /// # Errors
    /// [`ReplayError`] if the recording does not replay exactly.
    pub fn new(elf: &Elf, rec: &'r Recording, max_instructions: u64) -> Result<Self, ReplayError> {
        let (engine, start) = Engine::new(elf, rec)?;
        let verified = engine.verify(start.clone(), max_instructions)?;
        let end = verified.end.n;
        let interval = (end / MAX_CHECKPOINTS)
            .next_power_of_two()
            .max(MIN_INTERVAL);
        let mut plan = verified.plan;
        let mut checkpoints = Vec::new();
        let mut m = start;
        loop {
            engine.deliver(&mut m, &mut plan, false)?;
            if m.n % interval == 0 {
                checkpoints.push(m.clone());
            }
            if engine.at_end(&m) {
                break;
            }
            engine.execute(&mut m)?;
        }
        let now = checkpoints[0].clone();
        Ok(Timeline {
            engine,
            plan,
            checkpoints,
            interval,
            end,
            now,
            breakpoints: BTreeSet::new(),
            watchpoints: Vec::new(),
        })
    }

    /// The current position.
    #[must_use]
    pub fn position(&self) -> u64 {
        self.now.n
    }

    /// The last position: the end of the recording.
    #[must_use]
    pub fn end(&self) -> u64 {
        self.end
    }

    /// Instructions between checkpoints.
    #[must_use]
    pub fn interval(&self) -> u64 {
        self.interval
    }

    /// The machine at the current position.
    #[must_use]
    pub fn cpu(&self) -> &Cpu {
        &self.now.cpu
    }

    /// Recorded interrupts delivered up to the current position.
    #[must_use]
    pub fn interrupts_delivered(&self) -> usize {
        self.now.irqs
    }

    /// Bytes of memory at the current position; `None` for bytes outside
    /// the emulated memory (peripherals among them).
    #[must_use]
    pub fn read_memory(&self, address: u32, len: u32) -> Vec<Option<u8>> {
        (0..len)
            .map(|i| {
                self.now
                    .cpu
                    .memory
                    .peek(address.wrapping_add(i), 1)
                    .map(|b| b as u8)
            })
            .collect()
    }

    /// Stop when the instruction at `address` is reached.
    pub fn add_breakpoint(&mut self, address: u32) {
        self.breakpoints.insert(address & !1);
    }

    /// Remove a breakpoint.
    pub fn remove_breakpoint(&mut self, address: u32) {
        self.breakpoints.remove(&(address & !1));
    }

    /// Stop on the accesses `w` names.
    pub fn add_watchpoint(&mut self, w: Watchpoint) {
        self.watchpoints.push(w);
        arm(&mut self.now, &self.watchpoints);
    }

    /// Remove a watchpoint given to [`Timeline::add_watchpoint`].
    pub fn remove_watchpoint(&mut self, w: Watchpoint) {
        if let Some(i) = self.watchpoints.iter().position(|x| *x == w) {
            self.watchpoints.remove(i);
        }
        arm(&mut self.now, &self.watchpoints);
    }

    /// Go to position `p` (at most the end).
    ///
    /// # Errors
    /// [`ReplayError`] if the replay fails, which a checked recording does
    /// not.
    pub fn seek(&mut self, p: u64) -> Result<(), ReplayError> {
        let p = p.min(self.end);
        let i = usize::try_from(p / self.interval).unwrap_or(usize::MAX);
        let checkpoint = &self.checkpoints[i.min(self.checkpoints.len() - 1)];
        if p < self.now.n || checkpoint.n > self.now.n {
            self.now = checkpoint.clone();
        }
        self.now.cpu.memory.unwatch_all();
        while self.now.n < p {
            self.advance()?;
        }
        arm(&mut self.now, &self.watchpoints);
        Ok(())
    }

    /// Execute one instruction, delivering the interrupts due after it.
    fn advance(&mut self) -> Result<(), ReplayError> {
        self.engine.execute(&mut self.now)?;
        self.engine.deliver(&mut self.now, &mut self.plan, false)
    }

    /// One instruction forwards ([`Stop::End`] if there is none). Returns
    /// [`Stop::Watch`] if it made a watched access.
    ///
    /// # Errors
    /// See [`Timeline::seek`].
    pub fn step(&mut self) -> Result<Stop, ReplayError> {
        if self.engine.at_end(&self.now) {
            return Ok(Stop::End);
        }
        let _ = self.now.cpu.memory.take_watch_hit();
        self.advance()?;
        Ok(match self.now.cpu.memory.take_watch_hit() {
            Some(hit) => Stop::Watch(hit),
            None => Stop::Step,
        })
    }

    /// One instruction backwards ([`Stop::Start`] if there is none).
    ///
    /// # Errors
    /// See [`Timeline::seek`].
    pub fn step_back(&mut self) -> Result<Stop, ReplayError> {
        if self.now.n == 0 {
            return Ok(Stop::Start);
        }
        self.seek(self.now.n - 1)?;
        Ok(Stop::Step)
    }

    /// Run forwards until a breakpoint is reached, a watched access is
    /// made, the end is reached, or `interrupt` says to stop.
    ///
    /// # Errors
    /// See [`Timeline::seek`].
    pub fn run(&mut self, interrupt: &mut dyn FnMut() -> bool) -> Result<Stop, ReplayError> {
        let mut steps = 0u64;
        loop {
            match self.step()? {
                Stop::Step => {}
                stop => return Ok(stop),
            }
            if self.breakpoints.contains(&self.now.cpu.r[15]) {
                return Ok(Stop::Breakpoint);
            }
            steps += 1;
            if steps % POLL_EVERY == 0 && interrupt() {
                return Ok(Stop::Interrupted);
            }
        }
    }

    /// Run backwards to the most recent earlier position where a
    /// breakpoint address is reached, or before the most recent earlier
    /// instruction that made a watched access; or to the start.
    ///
    /// # Errors
    /// See [`Timeline::seek`].
    pub fn run_back(&mut self, interrupt: &mut dyn FnMut() -> bool) -> Result<Stop, ReplayError> {
        let mut upto = self.now.n; // positions before this one are searched
        while upto > 0 {
            let from = (upto - 1) / self.interval * self.interval;
            let i = usize::try_from(from / self.interval).unwrap_or(usize::MAX);
            let mut m = self.checkpoints[i].clone();
            arm(&mut m, &self.watchpoints);
            let mut found = None;
            while m.n < upto {
                let q = m.n;
                if self.breakpoints.contains(&m.cpu.r[15]) {
                    found = Some((q, Stop::Breakpoint));
                }
                let _ = m.cpu.memory.take_watch_hit();
                self.engine.execute(&mut m)?;
                self.engine.deliver(&mut m, &mut self.plan, false)?;
                if let Some(hit) = m.cpu.memory.take_watch_hit() {
                    found = Some((q, Stop::Watch(hit)));
                }
            }
            if let Some((q, stop)) = found {
                self.seek(q)?;
                return Ok(stop);
            }
            upto = from;
            if interrupt() {
                self.seek(upto)?;
                return Ok(Stop::Interrupted);
            }
        }
        self.seek(0)?;
        Ok(Stop::Start)
    }
}

/// Make the machine's memory report the watched accesses.
fn arm(m: &mut Machine, watchpoints: &[Watchpoint]) {
    m.cpu.memory.unwatch_all();
    for w in watchpoints {
        m.cpu.memory.watch(w.address, w.len, w.kind);
    }
}
