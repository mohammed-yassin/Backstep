//! The ARMv7-M exception model (Architecture Reference Manual, DDI 0403E,
//! chapter B1.5) and the parts of the System Control Space that drive it
//! (B3.2 System Control Block, B3.4 NVIC).
//!
//! Exceptions are taken at instruction boundaries: a synchronous one (a
//! fault or SVC) instead of the instruction that caused it, an
//! asynchronous one (pended by software or, during replay, by the
//! recording) after the instruction that made it pending and able to
//! preempt. Tail-chaining is a return followed directly by the next entry,
//! which is architecturally identical.
//!
//! Peripherals that are clocks (SysTick, DWT) are not modelled here: their
//! registers go to the [`Bus`], which during replay answers from the
//! recording.

use super::{vfp, Cpu, Exec, Stop};
use crate::alu::Flags;
use crate::memory::{Bus, Fault};

/// NMI.
pub const NMI: usize = 2;
/// HardFault.
pub const HARD_FAULT: usize = 3;
/// MemManage fault.
pub const MEM_MANAGE: usize = 4;
/// BusFault.
pub const BUS_FAULT: usize = 5;
/// UsageFault.
pub const USAGE_FAULT: usize = 6;
/// SVCall.
pub const SVCALL: usize = 11;
/// Debug monitor.
pub const DEBUG_MONITOR: usize = 12;
/// `PendSV`.
pub const PENDSV: usize = 14;
/// `SysTick`.
pub const SYSTICK: usize = 15;
/// Exception number of external interrupt 0.
pub const IRQ0: usize = 16;

/// CFSR bits (B3.2.15).
pub mod cfsr {
    /// Instruction bus error.
    pub const IBUSERR: u32 = 1 << 8;
    /// Precise data bus error.
    pub const PRECISERR: u32 = 1 << 9;
    /// BFAR holds the faulting address.
    pub const BFARVALID: u32 = 1 << 15;
    /// Undefined instruction.
    pub const UNDEFINSTR: u32 = 1 << 16;
    /// Execution with EPSR.T = 0.
    pub const INVSTATE: u32 = 1 << 17;
    /// Coprocessor instruction with the coprocessor absent or disabled.
    pub const NOCP: u32 = 1 << 19;
    /// Unaligned access.
    pub const UNALIGNED: u32 = 1 << 24;
    /// Division by zero (with CCR.DIV_0_TRP).
    pub const DIVBYZERO: u32 = 1 << 25;
}

/// CCR bits (B3.2.8).
pub mod ccr {
    /// Thread mode may be entered with exceptions active.
    pub const NONBASETHRDENA: u32 = 1 << 0;
    /// Unprivileged code may write STIR.
    pub const USERSETMPEND: u32 = 1 << 1;
    /// Trap unaligned word and halfword accesses.
    pub const UNALIGN_TRP: u32 = 1 << 3;
    /// Trap division by zero.
    pub const DIV_0_TRP: u32 = 1 << 4;
    /// Ignore data bus faults at priority -1 and -2.
    pub const BFHFNMIGN: u32 = 1 << 8;
    /// 8-byte stack alignment on exception entry.
    pub const STKALIGN: u32 = 1 << 9;
    /// All writable bits.
    pub const WRITABLE: u32 =
        NONBASETHRDENA | USERSETMPEND | UNALIGN_TRP | DIV_0_TRP | BFHFNMIGN | STKALIGN;
}

/// Exception state and the System Control Block / NVIC registers that
/// hold it. Indexed by exception number.
#[derive(Debug, Clone)]
pub struct Scs {
    /// Number of external interrupts implemented.
    pub num_irqs: usize,
    /// Pending state.
    pub pending: Vec<bool>,
    /// Active state.
    pub active: Vec<bool>,
    /// Enable state of external interrupts (NVIC ISER).
    pub enabled: Vec<bool>,
    /// Configurable priorities (SHPR, NVIC IPR), already masked to the
    /// implemented bits.
    pub priority: Vec<u8>,
    /// VTOR.
    pub vtor: u32,
    /// AIRCR.PRIGROUP.
    pub prigroup: u32,
    /// SCR.
    pub scr: u32,
    /// CCR (reset: STKALIGN set, as on Cortex-M4 r0p1).
    pub ccr: u32,
    /// SHCSR enable bits (MEMFAULTENA, BUSFAULTENA, USGFAULTENA).
    pub shcsr_enable: u32,
    /// CFSR.
    pub cfsr: u32,
    /// HFSR.
    pub hfsr: u32,
    /// MMFAR.
    pub mmfar: u32,
    /// BFAR.
    pub bfar: u32,
    /// CPACR (only the cp10 and cp11 fields).
    pub cpacr: u32,
    /// FPCCR (reset: ASPEN and LSPEN set).
    pub fpccr: u32,
    /// FPCAR: where lazy state preservation writes S0-S15 and FPSCR.
    pub fpcar: u32,
    /// FPDSCR: the FPSCR controls of a new floating-point context.
    pub fpdscr: u32,
}

impl Scs {
    /// Reset state for a core with `num_irqs` external interrupts.
    #[must_use]
    pub fn new(num_irqs: usize) -> Self {
        let n = IRQ0 + num_irqs;
        Scs {
            num_irqs,
            pending: vec![false; n],
            active: vec![false; n],
            enabled: vec![false; n],
            priority: vec![0; n],
            vtor: 0,
            prigroup: 0,
            scr: 0,
            ccr: ccr::STKALIGN,
            shcsr_enable: 0,
            cfsr: 0,
            hfsr: 0,
            mmfar: 0,
            bfar: 0,
            cpacr: 0,
            fpccr: vfp::fpccr::RESET,
            fpcar: 0,
            fpdscr: 0,
        }
    }

    fn exceptions(&self) -> usize {
        IRQ0 + self.num_irqs
    }
}

/// Counters of exception events, for test coverage and for the user.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExceptionStats {
    /// Entries per exception number.
    pub entries: Vec<u64>,
    /// Entries that preempted another handler.
    pub nested: u64,
    /// Entries straight after a return (tail-chaining).
    pub tail_chained: u64,
    /// Faults and SVCs escalated to `HardFault`.
    pub escalated: u64,
    /// Frames realigned to 8 bytes (xPSR bit 9).
    pub realigned: u64,
    /// Frames pushed onto the process stack.
    pub process_stack: u64,
    /// Returns to handler mode, thread mode on MSP, thread mode on PSP.
    pub returns: [u64; 3],
    /// Faults by cause, in the order of [`FAULT_CAUSES`].
    pub faults: [u64; 7],
    /// Entries with an extended (floating-point) frame.
    pub fp_frames: u64,
    /// Lazy floating-point state preservations (an FP instruction with
    /// FPCCR.LSPACT set).
    pub fp_lazy_saves: u64,
    /// Returns from an extended frame whose FP registers were never
    /// written (LSPACT still set).
    pub fp_lazy_returns: u64,
    /// Returns from an extended frame that loaded the FP registers.
    pub fp_restores: u64,
}

/// Fault causes counted in [`ExceptionStats::faults`], with their CFSR bit.
pub const FAULT_CAUSES: [(&str, u32); 7] = [
    ("undefinstr", cfsr::UNDEFINSTR),
    ("invstate", cfsr::INVSTATE),
    ("nocp", cfsr::NOCP),
    ("unaligned", cfsr::UNALIGNED),
    ("divbyzero", cfsr::DIVBYZERO),
    ("ibuserr", cfsr::IBUSERR),
    ("preciserr", cfsr::PRECISERR),
];

/// The state of an exception return at its tail-chaining check, after
/// the returning exception was deactivated and before unstacking.
#[derive(Debug, Clone)]
pub(super) struct ReturnPoint {
    regs: super::Registers,
    ipsr: u32,
    s: [u32; 32],
    fpscr: u32,
    fpccr: u32,
    pc: u32,
    value: u32,
    kind: usize,
}

/// Fixed priorities: Reset -3, NMI -2, `HardFault` -1.
fn fixed_priority(n: usize) -> Option<i32> {
    match n {
        1 => Some(-3),
        NMI => Some(-2),
        HARD_FAULT => Some(-1),
        _ => None,
    }
}

fn has_priority_register(n: usize) -> bool {
    matches!(
        n,
        MEM_MANAGE | BUS_FAULT | USAGE_FAULT | SVCALL | DEBUG_MONITOR | PENDSV | SYSTICK
    ) || n >= IRQ0
}

fn unsupported(pc: u32, what: &'static str) -> Stop {
    Stop::Unsupported { pc, what }
}

impl Cpu {
    /// Make exception `n` pending (an interrupt request, or `PendSV` /
    /// `SysTick` / NMI). It is taken at the next instruction boundary at
    /// which it can preempt.
    ///
    /// # Panics
    /// If `n` is not an implemented exception number.
    pub fn set_pending(&mut self, n: usize) {
        assert!(n >= NMI && n < self.scs.exceptions(), "no exception {n}");
        self.scs.pending[n] = true;
    }

    /// Make interrupt `n` pending and take it now, before the next
    /// instruction, if it can preempt (how replay delivers a recorded
    /// interrupt at its recorded position). Returns whether `n` was entered.
    ///
    /// # Errors
    /// A [`Stop`] from exception entry (stacking fault, vector read).
    ///
    /// # Panics
    /// If `n` is not an implemented exception number.
    pub fn interrupt_now(&mut self, bus: &mut dyn Bus, n: usize) -> Result<bool, Stop> {
        let was_pending = self.scs.pending[n];
        self.set_pending(n);
        let preempts = self.highest_pending() == Some(n)
            && self.group(self.priority(n)) < self.execution_priority();
        if !preempts {
            self.scs.pending[n] = was_pending;
            return Ok(false);
        }
        let ret = self.r[15];
        self.take_exception(bus, n, ret)?;
        Ok(true)
    }

    /// Priority of exception `n` (-3..-1 fixed, else the register value).
    fn priority(&self, n: usize) -> i32 {
        fixed_priority(n).unwrap_or_else(|| i32::from(self.scs.priority[n]))
    }

    /// Group priority of a priority value (B1.5.4 `GroupPriority`).
    fn group(&self, priority: i32) -> i32 {
        if priority < 0 {
            priority
        } else {
            priority & !((2 << self.scs.prigroup) - 1)
        }
    }

    /// `ExecutionPriority()` (B1.5.4): -2 in NMI, -1 in `HardFault` or
    /// with FAULTMASK, else the most urgent active exception or mask, 256
    /// when nothing boosts it.
    #[must_use]
    pub fn execution_priority(&self) -> i32 {
        let mut highest = 256;
        for n in 1..self.scs.exceptions() {
            if self.scs.active[n] {
                highest = highest.min(self.group(self.priority(n)));
            }
        }
        let mut boosted = 256;
        if self.basepri != 0 {
            boosted = self.group(i32::from(self.basepri));
        }
        if self.primask {
            boosted = 0;
        }
        if self.faultmask {
            boosted = -1;
        }
        highest.min(boosted)
    }

    fn takeable(&self, n: usize) -> bool {
        self.scs.pending[n]
            && match n {
                MEM_MANAGE => self.scs.shcsr_enable & (1 << 16) != 0,
                BUS_FAULT => self.scs.shcsr_enable & (1 << 17) != 0,
                USAGE_FAULT => self.scs.shcsr_enable & (1 << 18) != 0,
                n if n >= IRQ0 => self.scs.enabled[n],
                _ => true,
            }
    }

    /// The pending exception that would be taken next if it could preempt:
    /// lowest priority value, then lowest exception number.
    fn highest_pending(&self) -> Option<usize> {
        (NMI..self.scs.exceptions())
            .filter(|&n| self.takeable(n))
            .min_by_key(|&n| (self.priority(n), n))
    }

    /// Take the highest pending exception if it can preempt.
    pub(super) fn take_pending(&mut self, bus: &mut dyn Bus) -> Exec {
        if let Some(n) = self.highest_pending() {
            if self.group(self.priority(n)) < self.execution_priority() {
                let ret = self.r[15];
                return self.take_exception(bus, n, ret);
            }
        }
        Ok(())
    }

    /// A synchronous exception (fault or SVC) with its CFSR bits: taken if
    /// enabled and higher than the execution priority, else escalated to
    /// `HardFault`, else lockup (B1.5.11, priority escalation).
    pub(super) fn raise(&mut self, bus: &mut dyn Bus, n: usize, ret: u32, cfsr: u32) -> Exec {
        let execution = self.execution_priority();
        let enabled = match n {
            MEM_MANAGE => self.scs.shcsr_enable & (1 << 16) != 0,
            BUS_FAULT => self.scs.shcsr_enable & (1 << 17) != 0,
            USAGE_FAULT => self.scs.shcsr_enable & (1 << 18) != 0,
            _ => true,
        };
        self.scs.cfsr |= cfsr;
        for (i, (_, bit)) in FAULT_CAUSES.iter().enumerate() {
            if cfsr & bit != 0 {
                self.stats.faults[i] += 1;
            }
        }
        let target = if enabled && self.group(self.priority(n)) < execution {
            n
        } else if execution > -1 {
            self.scs.hfsr |= 1 << 30; // FORCED
            self.stats.escalated += 1;
            HARD_FAULT
        } else {
            return Err(Stop::Lockup { pc: ret });
        };
        self.take_exception(bus, target, ret)
    }

    /// Exception entry: `PushStack` and `ExceptionTaken` (B1.5.6). With a
    /// floating-point context active (CONTROL.FPCA) the frame is the
    /// extended one: 26 words, S0-S15 at +0x20, FPSCR at +0x60, a reserved
    /// word at +0x64. With FPCCR.LSPEN the space is only reserved and
    /// FPCCR records what lazy preservation needs; otherwise the registers
    /// are stored now and then, as QEMU does, cleared.
    fn take_exception(&mut self, bus: &mut dyn Bus, n: usize, ret: u32) -> Exec {
        self.last_return = None;
        let fp = self.control & 4 != 0;
        let use_psp = self.using_psp();
        let sp = self.r[13];
        let realign = self.scs.ccr & ccr::STKALIGN != 0 && sp & 4 != 0;
        let size = if fp { 0x68 } else { 0x20 };
        let frame = sp.wrapping_sub(size) & !(u32::from(realign) << 2);
        let xpsr = (self.xpsr() & !(1 << 9)) | (u32::from(realign) << 9);
        let words = [
            self.r[0], self.r[1], self.r[2], self.r[3], self.r[12], self.r[14], ret, xpsr,
        ];
        for (i, v) in words.iter().enumerate() {
            if self
                .write(bus, ret, frame.wrapping_add(4 * i as u32), 4, *v)
                .is_err()
            {
                return Err(unsupported(ret, "fault while stacking (derived fault)"));
            }
        }
        if fp {
            self.stack_fp_state(bus, ret, frame.wrapping_add(0x20))?;
        }
        let vector_address = (self.scs.vtor & !3).wrapping_add(4 * n as u32);
        let Ok(vector) = self.read(bus, ret, vector_address, 4) else {
            return Err(unsupported(
                ret,
                "vector table read failed (HardFault VECTTBL)",
            ));
        };
        let in_handler = self.ipsr != 0;
        self.r[13] = frame;
        // EXC_RETURN bit 4 (FType) is clear for an extended frame.
        self.r[14] = if in_handler {
            0xFFFF_FFF1
        } else if use_psp {
            0xFFFF_FFFD
        } else {
            0xFFFF_FFF9
        } & !(u32::from(fp) << 4);
        if use_psp {
            std::mem::swap(&mut self.r[13], &mut self.sp_banked); // to MSP
        }
        self.control &= !(2 | 4); // SPSEL = 0 (handler mode uses MSP), FPCA = 0
        self.ipsr = n as u32;
        self.it = 0;
        self.thumb = vector & 1 != 0;
        self.r[15] = vector & !1;
        self.scs.active[n] = true;
        self.scs.pending[n] = false;
        self.monitor = None;
        if self.stats.entries.len() < self.scs.exceptions() {
            self.stats.entries.resize(self.scs.exceptions(), 0);
        }
        self.stats.entries[n] += 1;
        self.stats.nested += u64::from(in_handler);
        self.stats.realigned += u64::from(realign);
        self.stats.process_stack += u64::from(use_psp);
        Ok(())
    }

    /// The floating-point part of an extended frame at `area` (the frame
    /// + 0x20), during exception entry.
    fn stack_fp_state(&mut self, bus: &mut dyn Bus, ret: u32, area: u32) -> Exec {
        self.stats.fp_frames += 1;
        if self.scs.fpccr & vfp::fpccr::LSPEN != 0 {
            self.update_fpccr(area);
            return Ok(());
        }
        if !self.fp_enabled() {
            return Err(unsupported(
                ret,
                "CPACR forbids stacking the floating-point registers (NOCP on entry)",
            ));
        }
        let words: Vec<u32> = self.s[..16].iter().copied().chain([self.fpscr]).collect();
        for (i, v) in words.iter().enumerate() {
            if self
                .write(bus, ret, area.wrapping_add(4 * i as u32), 4, *v)
                .is_err()
            {
                return Err(unsupported(ret, "fault while stacking (derived fault)"));
            }
        }
        // The architecture leaves S0-S15 and FPSCR UNKNOWN; QEMU clears them.
        self.s[..16].fill(0);
        self.fpscr = 0;
        Ok(())
    }

    /// `UpdateFPCCR`, at an exception entry that reserves the floating-point
    /// part of an extended frame at `area` (public for replay, which
    /// reproduces exceptions the recorder takes without running them):
    /// remember where lazy preservation must write, and the
    /// background context's privilege, mode and which faults are ready
    /// (could preempt it), as QEMU's `v7m_update_fpccr`.
    pub fn update_fpccr(&mut self, area: u32) {
        use vfp::fpccr::{BFRDY, HFRDY, LSPACT, MMRDY, MONRDY, THREAD, UFRDY, USER};
        let execution = self.execution_priority();
        let ready = |n: usize, enable: u32| {
            self.scs.shcsr_enable & enable != 0 && self.group(self.priority(n)) < execution
        };
        let bits = [
            (!self.privileged(), USER),
            (self.ipsr == 0, THREAD),
            (execution > -1, HFRDY),
            (ready(MEM_MANAGE, 1 << 16), MMRDY),
            (ready(BUS_FAULT, 1 << 17), BFRDY),
            (ready(USAGE_FAULT, 1 << 18), UFRDY),
        ];
        let mut f = self.scs.fpccr & !(USER | THREAD | HFRDY | MMRDY | BFRDY | MONRDY | UFRDY);
        f |= LSPACT;
        for (set, bit) in bits {
            if set {
                f |= bit;
            }
        }
        // MONRDY: the DebugMonitor is never enabled here (no DEMCR).
        self.scs.fpccr = f;
        self.scs.fpcar = area & !7;
    }

    /// Exception return (`ExceptionReturn`, `PopStack`; B1.5.8), after the
    /// instruction that loaded `EXC_RETURN` into the PC. Integrity-check
    /// failures (UsageFault INVPC) are reported as unsupported: they only
    /// happen in broken firmware, and their exact behaviour is part of M4.
    pub(super) fn exception_return(&mut self, bus: &mut dyn Bus, pc: u32, value: u32) -> Exec {
        if value & 0x0FFF_FFE0 != 0x0FFF_FFE0 {
            return Err(unsupported(pc, "INVPC: invalid EXC_RETURN"));
        }
        // FType: bit 4 clear means an extended (floating-point) frame.
        let extended = value & 0x10 == 0;
        let n = self.ipsr as usize;
        if n >= self.scs.exceptions() || !self.scs.active[n] {
            return Err(unsupported(pc, "INVPC: return from an inactive exception"));
        }
        let (to_thread, psp, kind) = match value & 0xF {
            0x1 => (false, false, 0),
            0x9 => (true, false, 1),
            0xD => (true, true, 2),
            _ => return Err(unsupported(pc, "INVPC: invalid EXC_RETURN")),
        };
        let nested = self.scs.active.iter().filter(|a| **a).count();
        if to_thread && nested != 1 && self.scs.ccr & ccr::NONBASETHRDENA == 0 {
            return Err(unsupported(
                pc,
                "INVPC: return to thread mode with exceptions active",
            ));
        }
        self.scs.active[n] = false;
        if n != NMI {
            self.faultmask = false;
        }
        self.monitor = None;
        // Tail-chaining (B1.5.12): if a pending exception can preempt the
        // context being returned to, it is entered at once, without
        // unstacking: the frame stays, registers keep the handler's values
        // and LR keeps this EXC_RETURN.
        if let Some(next) = self.highest_pending() {
            if self.group(self.priority(next)) < self.execution_priority() {
                return self.tail_chain(bus, pc, next, value);
            }
        }
        let point = ReturnPoint {
            regs: self.registers(),
            ipsr: self.ipsr,
            s: self.s,
            fpscr: self.fpscr,
            fpccr: self.scs.fpccr,
            pc,
            value,
            kind,
        };
        // In handler mode r13 is MSP and sp_banked is PSP.
        let frame = if psp { self.sp_banked } else { self.r[13] };
        let mut words = [0u32; 8];
        for (i, w) in words.iter_mut().enumerate() {
            match self.read(bus, pc, frame.wrapping_add(4 * i as u32), 4) {
                Ok(v) => *w = v,
                Err(_) => return Err(unsupported(pc, "fault while unstacking (derived fault)")),
            }
        }
        let psr = words[7];
        let frame_ipsr = psr & 0x1FF;
        if to_thread != (frame_ipsr == 0) {
            return Err(unsupported(
                pc,
                "INVPC: stacked IPSR does not match EXC_RETURN",
            ));
        }
        if extended {
            self.unstack_fp_state(bus, pc, frame.wrapping_add(0x20), to_thread)?;
        }
        let align = u32::from(psr & (1 << 9) != 0 && self.scs.ccr & ccr::STKALIGN != 0) << 2;
        let size = if extended { 0x68 } else { 0x20 };
        let new_sp = frame.wrapping_add(size) | align;
        for (reg, v) in [0usize, 1, 2, 3, 12, 14].iter().zip(words) {
            self.r[*reg] = v;
        }
        self.r[15] = words[6] & !1;
        self.flags = Flags::from_bits(psr);
        self.ge = ((psr >> 16) & 0xF) as u8;
        self.it = (((psr >> 25) & 3) | (((psr >> 10) & 0x3F) << 2)) as u8;
        self.thumb = psr & (1 << 24) != 0;
        self.ipsr = frame_ipsr;
        self.control = (self.control & !4) | (u32::from(extended) << 2);
        if psp {
            // Back to thread mode on the process stack.
            let msp = self.r[13];
            self.r[13] = new_sp;
            self.sp_banked = msp;
            self.control |= 2;
        } else {
            self.r[13] = new_sp;
            self.control &= !2;
        }
        self.stats.returns[kind] += 1;
        self.last_return = Some(Box::new(point));
        Ok(())
    }

    /// Right after an instruction that returned from an exception and
    /// unstacked: take exception `n` as if it had been pending at the
    /// return, so that it tail-chained (B1.5.12) instead: the return is
    /// undone (registers, floating-point registers and FPCCR as at its
    /// tail-chaining check) and `n` is entered on the same frame.
    ///
    /// Replay needs this because an interrupt that tail-chained and one
    /// that arrived right after the return leave the same integer state
    /// but different floating-point state (the registers are not
    /// restored on a tail-chain), and only the recording knows which
    /// happened. Returns false, with nothing changed, if the last
    /// instruction did not unstack, or if `n` could not have preempted the
    /// context returned to.
    ///
    /// # Errors
    /// As for exception entry.
    pub fn tail_chain_after_return(&mut self, bus: &mut dyn Bus, n: usize) -> Result<bool, Stop> {
        let Some(point) = self.last_return.as_deref().cloned() else {
            return Ok(false);
        };
        let mut c = self.clone();
        c.restore(point.regs);
        c.ipsr = point.ipsr;
        c.s = point.s;
        c.fpscr = point.fpscr;
        c.scs.fpccr = point.fpccr;
        c.set_pending(n);
        let preempts =
            c.highest_pending() == Some(n) && c.group(c.priority(n)) < c.execution_priority();
        if !preempts {
            return Ok(false);
        }
        c.stats.returns[point.kind] -= 1;
        c.last_return = None;
        c.tail_chain(bus, point.pc, n, point.value)?;
        *self = c;
        Ok(true)
    }

    /// The floating-point part of an extended frame at `area` during
    /// exception return: if lazy preservation never happened (LSPACT
    /// still set), the registers still hold the interrupted context's
    /// values and only LSPACT is cleared; otherwise they are loaded.
    fn unstack_fp_state(&mut self, bus: &mut dyn Bus, pc: u32, area: u32, to_thread: bool) -> Exec {
        use vfp::fpccr::LSPACT;
        if self.scs.fpccr & LSPACT != 0 {
            self.scs.fpccr &= !LSPACT;
            self.stats.fp_lazy_returns += 1;
            return Ok(());
        }
        // The context returned to must have access to the FPU.
        if !self.cpacr_allows(!to_thread || self.control & 1 == 0) {
            return Err(unsupported(
                pc,
                "CPACR forbids restoring the floating-point registers (NOCP on return)",
            ));
        }
        let mut words = [0u32; 17];
        for (i, w) in words.iter_mut().enumerate() {
            match self.read(bus, pc, area.wrapping_add(4 * i as u32), 4) {
                Ok(v) => *w = v,
                Err(_) => return Err(unsupported(pc, "fault while unstacking (derived fault)")),
            }
        }
        self.s[..16].copy_from_slice(&words[..16]);
        self.fpscr = words[16] & crate::fpu::fpscr::WRITABLE;
        self.stats.fp_restores += 1;
        Ok(())
    }

    /// Enter exception `n` straight from the return of the previous one.
    /// The frame stays as it is, floating-point part included; CONTROL.FPCA
    /// is cleared as on any entry.
    fn tail_chain(&mut self, bus: &mut dyn Bus, pc: u32, n: usize, exc_return: u32) -> Exec {
        let vector_address = (self.scs.vtor & !3).wrapping_add(4 * n as u32);
        let Ok(vector) = self.read(bus, pc, vector_address, 4) else {
            return Err(unsupported(
                pc,
                "vector table read failed (HardFault VECTTBL)",
            ));
        };
        self.r[14] = exc_return;
        self.control &= !4;
        self.ipsr = n as u32;
        self.it = 0;
        self.thumb = vector & 1 != 0;
        self.r[15] = vector & !1;
        self.scs.active[n] = true;
        self.scs.pending[n] = false;
        self.stats.entries[n] += 1;
        self.stats.tail_chained += 1;
        Ok(())
    }

    /// True for the addresses of the System Control Space registers this
    /// module implements (SCB and NVIC; not SysTick, CPUID or debug).
    pub(super) fn is_scs(address: u32) -> bool {
        matches!(address,
            0xE000_ED04..=0xE000_ED2F
            | 0xE000_ED34..=0xE000_ED3B
            | 0xE000_E100..=0xE000_E33F
            | 0xE000_E400..=0xE000_E5EF
            | 0xE000_ED88..=0xE000_ED8B
            | 0xE000_EF00..=0xE000_EF03
            | 0xE000_EF34..=0xE000_EF4B)
    }

    fn scs_fault(pc: u32, address: u32, size: u8, write: bool) -> Stop {
        Stop::BusFault {
            pc,
            fault: Fault {
                address,
                size,
                write,
            },
        }
    }

    fn priority_byte(&self, n: usize) -> u32 {
        if n < self.scs.exceptions() && has_priority_register(n) {
            u32::from(self.scs.priority[n])
        } else {
            0
        }
    }

    fn set_priority_byte(&mut self, n: usize, v: u32) {
        if n < self.scs.exceptions() && has_priority_register(n) {
            self.scs.priority[n] = (v as u8) & self.priority_mask;
        }
    }

    /// Exception numbers of the 32 interrupts in NVIC register `index`.
    fn irq_bits(&self, index: u32, of: &[bool]) -> u32 {
        (0..32)
            .filter(|b| {
                let n = IRQ0 + 32 * index as usize + b;
                n < self.scs.exceptions() && of[n]
            })
            .fold(0, |acc, b| acc | (1 << b))
    }

    fn update_irq_bits(&mut self, index: u32, value: u32, set: bool, pending: bool) {
        for b in 0..32 {
            let n = IRQ0 + 32 * index as usize + b;
            if value & (1 << b) != 0 && n < self.scs.exceptions() {
                if pending {
                    self.scs.pending[n] = set;
                } else {
                    self.scs.enabled[n] = set;
                }
            }
        }
    }

    fn icsr(&self) -> u32 {
        let active = self.scs.active.iter().filter(|a| **a).count();
        let vectpending = (NMI..self.scs.exceptions())
            .filter(|&n| self.takeable(n))
            .min_by_key(|&n| (self.group(self.priority(n)), self.priority(n), n))
            .unwrap_or(0) as u32;
        let isrpending = (IRQ0..self.scs.exceptions()).any(|n| self.scs.pending[n]);
        self.ipsr
            | u32::from(active <= 1) << 11
            | (vectpending & 0x1FF) << 12
            | u32::from(isrpending) << 22
            | u32::from(self.scs.pending[SYSTICK]) << 26
            | u32::from(self.scs.pending[PENDSV]) << 28
            | u32::from(self.scs.pending[NMI]) << 31
    }

    fn shcsr(&self) -> u32 {
        let a = &self.scs.active;
        let p = &self.scs.pending;
        u32::from(a[MEM_MANAGE])
            | u32::from(a[BUS_FAULT]) << 1
            | u32::from(a[USAGE_FAULT]) << 3
            | u32::from(a[SVCALL]) << 7
            | u32::from(a[DEBUG_MONITOR]) << 8
            | u32::from(a[PENDSV]) << 10
            | u32::from(a[SYSTICK]) << 11
            | u32::from(p[USAGE_FAULT]) << 12
            | u32::from(p[MEM_MANAGE]) << 13
            | u32::from(p[BUS_FAULT]) << 14
            | u32::from(p[SVCALL]) << 15
            | self.scs.shcsr_enable
    }

    /// Load from a System Control Space register.
    pub(super) fn scs_read(&mut self, pc: u32, address: u32, size: u8) -> Result<u32, Stop> {
        if !self.privileged() {
            return Err(Self::scs_fault(pc, address, size, false));
        }
        let word = address & !3;
        let shift = 8 * (address & 3);
        let value = match word {
            0xE000_ED18..=0xE000_ED23 | 0xE000_E400..=0xE000_E5EF => {
                // Byte-accessible priority registers.
                let (base, first) = if word >= 0xE000_ED00 {
                    (0xE000_ED18, MEM_MANAGE)
                } else {
                    (0xE000_E400, IRQ0)
                };
                let n0 = first + (word - base) as usize;
                (0..4).fold(0, |acc, i| acc | self.priority_byte(n0 + i) << (8 * i))
            }
            0xE000_ED04 => self.icsr(),
            0xE000_ED08 => self.scs.vtor,
            0xE000_ED0C => 0xFA05_0000 | self.scs.prigroup << 8,
            0xE000_ED10 => self.scs.scr,
            0xE000_ED14 => self.scs.ccr,
            0xE000_ED24 => self.shcsr(),
            0xE000_ED28 => self.scs.cfsr,
            0xE000_ED2C => self.scs.hfsr,
            0xE000_ED34 => self.scs.mmfar,
            0xE000_ED38 => self.scs.bfar,
            0xE000_ED88 => self.scs.cpacr,
            0xE000_EF34 => self.scs.fpccr,
            0xE000_EF38 => self.scs.fpcar,
            0xE000_EF3C => self.scs.fpdscr,
            0xE000_EF40 => vfp::MVFR0,
            0xE000_EF44 => vfp::MVFR1,
            0xE000_E100..=0xE000_E1FF => self.irq_bits((word >> 2) & 0x1F, &self.scs.enabled),
            0xE000_E200..=0xE000_E2FF => self.irq_bits((word >> 2) & 0x1F, &self.scs.pending),
            0xE000_E300..=0xE000_E33F => self.irq_bits((word >> 2) & 0xF, &self.scs.active),
            _ => 0, // STIR is write-only
        };
        let mask = if size == 4 {
            u32::MAX
        } else {
            (1 << (8 * u32::from(size))) - 1
        };
        Ok((value >> shift) & mask)
    }

    /// Store to a System Control Space register.
    pub(super) fn scs_write(&mut self, pc: u32, address: u32, size: u8, value: u32) -> Exec {
        let stir = (0xE000_EF00..=0xE000_EF03).contains(&address);
        let user_stir = stir && self.scs.ccr & ccr::USERSETMPEND != 0;
        if !self.privileged() && !user_stir {
            return Err(Self::scs_fault(pc, address, size, true));
        }
        if let 0xE000_ED18..=0xE000_ED23 | 0xE000_E400..=0xE000_E5EF = address {
            let (base, first) = if address >= 0xE000_ED00 {
                (0xE000_ED18, MEM_MANAGE)
            } else {
                (0xE000_E400, IRQ0)
            };
            let n0 = first + (address - base) as usize;
            for i in 0..usize::from(size) {
                self.set_priority_byte(n0 + i, value >> (8 * i));
            }
            return Ok(());
        }
        if size != 4 || address & 3 != 0 {
            return Err(unsupported(
                pc,
                "sub-word access to a word-only SCS register",
            ));
        }
        let v = value;
        match address {
            0xE000_ED04 => {
                if v & (1 << 31) != 0 {
                    self.scs.pending[NMI] = true;
                }
                if v & (1 << 28) != 0 {
                    self.scs.pending[PENDSV] = true;
                } else if v & (1 << 27) != 0 {
                    self.scs.pending[PENDSV] = false;
                }
                if v & (1 << 26) != 0 {
                    self.scs.pending[SYSTICK] = true;
                } else if v & (1 << 25) != 0 {
                    self.scs.pending[SYSTICK] = false;
                }
            }
            0xE000_ED08 => self.scs.vtor = v & 0xFFFF_FF80,
            0xE000_ED0C => {
                if v >> 16 == 0x05FA {
                    if v & 4 != 0 {
                        return Err(unsupported(pc, "SYSRESETREQ (system reset)"));
                    }
                    self.scs.prigroup = (v >> 8) & 7;
                }
            }
            0xE000_ED10 => self.scs.scr = v & 0x16,
            0xE000_ED14 => self.scs.ccr = v & ccr::WRITABLE,
            0xE000_ED24 => {
                let bits = [
                    (0, MEM_MANAGE, true),
                    (1, BUS_FAULT, true),
                    (3, USAGE_FAULT, true),
                    (7, SVCALL, true),
                    (8, DEBUG_MONITOR, true),
                    (10, PENDSV, true),
                    (11, SYSTICK, true),
                    (12, USAGE_FAULT, false),
                    (13, MEM_MANAGE, false),
                    (14, BUS_FAULT, false),
                    (15, SVCALL, false),
                ];
                for (bit, n, active) in bits {
                    let set = v & (1 << bit) != 0;
                    if active {
                        self.scs.active[n] = set;
                    } else {
                        self.scs.pending[n] = set;
                    }
                }
                self.scs.shcsr_enable = v & (7 << 16);
            }
            0xE000_ED28 => self.scs.cfsr &= !v,
            0xE000_ED2C => self.scs.hfsr &= !(v & 0xC000_0002),
            0xE000_ED34 => self.scs.mmfar = v,
            0xE000_ED38 => self.scs.bfar = v,
            0xE000_ED88 => self.scs.cpacr = v & vfp::CPACR_WRITABLE,
            0xE000_EF34 => self.scs.fpccr = v & vfp::fpccr::WRITABLE,
            0xE000_EF38 => self.scs.fpcar = v & !7,
            0xE000_EF3C => self.scs.fpdscr = v & vfp::FPDSCR_WRITABLE,
            0xE000_E100..=0xE000_E17F => {
                self.update_irq_bits((address >> 2) & 0x1F, v, true, false);
            }
            0xE000_E180..=0xE000_E1FF => {
                self.update_irq_bits((address >> 2) & 0x1F, v, false, false);
            }
            0xE000_E200..=0xE000_E27F => self.update_irq_bits((address >> 2) & 0x1F, v, true, true),
            0xE000_E280..=0xE000_E2FF => {
                self.update_irq_bits((address >> 2) & 0x1F, v, false, true);
            }
            0xE000_EF00 => {
                let n = IRQ0 + (v & 0x1FF) as usize;
                if n < self.scs.exceptions() {
                    self.scs.pending[n] = true;
                }
            }
            _ => {} // IABR, MVFR0-2 are read-only
        }
        Ok(())
    }
}
