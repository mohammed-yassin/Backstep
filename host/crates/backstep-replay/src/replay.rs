//! Re-run a recording on the emulator and check it.

use std::collections::{BTreeMap, VecDeque};
use std::fmt;
use std::io::Write;

use backstep_core::crc32::{crc32, update};
use backstep_emu::cpu::vfp;
use backstep_emu::elf::Elf;
use backstep_emu::memory::NoBus;
use backstep_emu::memory::{Bus, Fault};
use backstep_emu::{Cpu, Memory, Scs, Stop};

use crate::recording::{CpuState, FpState, IrqEvent, MmioEvent, Recording};
use crate::target::{self, Target};

/// The peripheral region the recorder captures (ADR 0003).
const PERIPHERALS: std::ops::Range<u32> = 0x4000_0000..0x6000_0000;
/// The Private Peripheral Bus (SysTick, MPU, debug: not recorded).
const PPB: std::ops::Range<u32> = 0xE000_0000..0xE010_0000;

/// Replay options.
pub struct Options<'a> {
    /// Stop with an error after this many instructions.
    pub max_instructions: u64,
    /// Write the state before every instruction here, in the format of
    /// `qemu-system-arm -d cpu` (for comparison with QEMU).
    pub trace: Option<&'a mut dyn Write>,
}

impl Default for Options<'_> {
    fn default() -> Self {
        Options {
            max_instructions: 100_000_000,
            trace: None,
        }
    }
}

/// A successful replay.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    /// The target.
    pub target: Target,
    /// Instructions executed.
    pub instructions: u64,
    /// Peripheral loads replayed (all of them).
    pub loads: usize,
    /// Peripheral stores (not recorded; replay ignores them).
    pub stores: u64,
    /// Interrupts delivered at their recorded positions (all of them).
    pub interrupts: usize,
    /// CRC-32 of the end RAM regions (equal to the recording's).
    pub ram_crc32: u32,
    /// Times replay went back to deliver an interrupt at a later match of
    /// its key, because an earlier match led to a divergence.
    pub retries: u32,
}

/// Why a replay failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReplayError {
    /// The recording names a target this tool does not know.
    UnknownTarget(u32),
    /// The ELF could not be loaded.
    Elf(String),
    /// The ELF is not the firmware that was recorded.
    FirmwareMismatch {
        /// CRC-32 in the recording.
        recorded: u32,
        /// CRC-32 of the ELF's image.
        elf: u32,
    },
    /// A peripheral load differs from the next recorded one.
    LoadMismatch {
        /// Instructions executed before it.
        instruction: u64,
        /// Index of the recorded load.
        index: usize,
        /// The recorded load.
        recorded: MmioEvent,
        /// The load replay made: (pc, address, size).
        actual: (u32, u32, u8),
    },
    /// More peripheral loads than were recorded.
    ExtraLoad {
        /// Instructions executed before it.
        instruction: u64,
        /// (pc, address, size).
        actual: (u32, u32, u8),
    },
    /// A load the recorder does not capture, whose value replay cannot
    /// know: from the PPB (e.g. SysTick or DWT), or from a peripheral at
    /// execution priority -1 or -2 (`HardFault`, NMI, FAULTMASK), where the
    /// MPU does not trap.
    UnrecordedLoad {
        /// Instructions executed before it.
        instruction: u64,
        /// (pc, address, size).
        actual: (u32, u32, u8),
    },
    /// The emulator stopped.
    Stopped {
        /// Instructions executed before it.
        instruction: u64,
        /// Why.
        stop: Stop,
    },
    /// The end point was not reached.
    InstructionLimit(u64),
    /// A recorded interrupt's position was reached but the emulated core
    /// could not take it (masked, or not the most urgent pending one).
    InterruptNotTaken {
        /// Instructions executed before it.
        instruction: u64,
        /// Index of the recorded interrupt.
        index: usize,
        /// The exception number.
        exception: u32,
    },
    /// The end point was reached with recorded interrupts not delivered:
    /// their positions were never reached.
    InterruptsLeft {
        /// Interrupts delivered.
        delivered: usize,
        /// Interrupts recorded.
        recorded: usize,
        /// The first one not delivered.
        next: IrqEvent,
    },
    /// The end point was reached with recorded loads left over.
    LoadsLeft {
        /// Loads replayed.
        replayed: usize,
        /// Loads recorded.
        recorded: usize,
    },
    /// At a recorded interrupt, FPCCR differs from the one the stub
    /// recorded: the floating-point context diverged.
    FpccrMismatch {
        /// Instructions executed before it.
        instruction: u64,
        /// Index of the recorded interrupt.
        index: usize,
        /// FPCCR recorded at the stub's entry.
        recorded: u32,
        /// FPCCR in the replay.
        replayed: u32,
    },
    /// The end state differs from the recording.
    EndMismatch {
        /// What differs.
        what: String,
        /// Recorded value.
        recorded: u32,
        /// Replayed value.
        replayed: u32,
    },
}

impl fmt::Display for ReplayError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let access = |(pc, address, size): (u32, u32, u8)| {
            format!("{size}-byte load from {address:#010x} at pc {pc:#010x}")
        };
        match self {
            ReplayError::UnknownTarget(id) => write!(f, "unknown target id {id}"),
            ReplayError::Elf(e) => write!(f, "cannot load the ELF: {e}"),
            ReplayError::FirmwareMismatch { recorded, elf } => write!(
                f,
                "the ELF is not the recorded firmware (image CRC-32 {elf:#010x}, recorded {recorded:#010x})"
            ),
            ReplayError::LoadMismatch {
                instruction,
                index,
                recorded,
                actual,
            } => write!(
                f,
                "diverged at instruction {instruction}: {} but recorded load #{index} is a {}-byte load from {:#010x} at pc {:#010x}",
                access(*actual),
                recorded.size,
                recorded.address,
                recorded.pc
            ),
            ReplayError::ExtraLoad {
                instruction,
                actual,
            } => write!(
                f,
                "diverged at instruction {instruction}: {} after the last recorded load",
                access(*actual)
            ),
            ReplayError::UnrecordedLoad {
                instruction,
                actual,
            } => write!(
                f,
                "at instruction {instruction}: {} is not recorded (not a captured \
                 peripheral, or made at priority -1 or -2, which the MPU does not trap)",
                access(*actual)
            ),
            ReplayError::Stopped { instruction, stop } => {
                write!(f, "the emulator stopped at instruction {instruction}: {stop:?}")
            }
            ReplayError::InstructionLimit(n) => {
                write!(f, "the end of the recording was not reached in {n} instructions")
            }
            ReplayError::InterruptNotTaken {
                instruction,
                index,
                exception,
            } => write!(
                f,
                "diverged at instruction {instruction}: recorded interrupt #{index} (exception {exception}) cannot be taken there"
            ),
            ReplayError::InterruptsLeft {
                delivered,
                recorded,
                next,
            } => write!(
                f,
                "diverged: the end was reached after {delivered} of {recorded} recorded interrupts; the position of the next (exception {}, pc {:#010x}) was never reached",
                next.exception, next.pc
            ),
            ReplayError::LoadsLeft { replayed, recorded } => write!(
                f,
                "diverged: the end was reached after {replayed} of {recorded} recorded loads"
            ),
            ReplayError::FpccrMismatch {
                instruction,
                index,
                recorded,
                replayed,
            } => write!(
                f,
                "diverged at instruction {instruction}: at recorded interrupt #{index} FPCCR is {replayed:#010x}, recorded {recorded:#010x} (the floating-point context differs)"
            ),
            ReplayError::EndMismatch {
                what,
                recorded,
                replayed,
            } => write!(
                f,
                "diverged: at the end {what} is {replayed:#010x}, recorded {recorded:#010x}"
            ),
        }
    }
}

impl std::error::Error for ReplayError {}

/// Answers peripheral loads from the recording.
struct ReplayBus<'a> {
    events: &'a [MmioEvent],
    next: usize,
    pc: u32,
    instruction: u64,
    stores: u64,
    /// Peripheral accesses made by the current instruction.
    accesses: u32,
    /// Whether the MPU traps accesses at the current execution priority.
    trapped: bool,
    error: Option<ReplayError>,
}

/// Dead stack the capture handler zeroes below its stack top
/// (`BS_TRAP_STACK_BYTES` in `backstep_mmio.h`).
const TRAP_STACK_BYTES: u32 = 520;

/// On the chip every peripheral access raised the capture's MemManage (or
/// HardFault) exception, which leaves its exception frame below SP, with
/// the registers as they are after the access, and zeros below that.
/// Replay does not run the capture, so it writes the same bytes: replayed
/// memory then equals the chip's, dead stack included, which interrupt
/// positions (their stack hash) depend on.
fn leave_trap_frame(cpu: &mut Cpu, frame_on_psp: bool) -> u32 {
    let sp = cpu.r[13];
    let realign = cpu.scs.ccr & (1 << 9) != 0 && sp & 4 != 0;
    // With a floating-point context the frame is the extended one (26
    // words), as the emulator's own exception entry makes it.
    let fp = cpu.control & 4 != 0;
    let size = if fp { 0x68 } else { 0x20 };
    let frame = sp.wrapping_sub(size) & !(u32::from(realign) << 2);
    let xpsr = cpu.xpsr() | (u32::from(realign) << 9);
    let words = [
        cpu.r[0], cpu.r[1], cpu.r[2], cpu.r[3], cpu.r[12], cpu.r[14], cpu.r[15], xpsr,
    ];
    for (i, w) in words.iter().enumerate() {
        cpu.memory.poke(frame.wrapping_add(4 * i as u32), 4, *w);
    }
    if fp {
        let area = frame.wrapping_add(0x20);
        if cpu.scs.fpccr & vfp::fpccr::LSPEN != 0 {
            // Lazy: the entry only reserves the space and updates FPCCR;
            // the handler never uses the FPU (the recorder is built
            // without FPU code), so its return just clears LSPACT.
            cpu.update_fpccr(area);
            cpu.scs.fpccr &= !vfp::fpccr::LSPACT;
        } else {
            // Stored at once (and loaded back by the return).
            let regs: Vec<u32> = cpu.s[..16].iter().copied().chain([cpu.fpscr]).collect();
            for (i, w) in regs.iter().enumerate() {
                cpu.memory.poke(area.wrapping_add(4 * i as u32), 4, *w);
            }
        }
    }
    let top = if frame_on_psp { cpu.msp() } else { frame };
    for i in 0..TRAP_STACK_BYTES / 4 {
        cpu.memory
            .poke(top.wrapping_sub(TRAP_STACK_BYTES).wrapping_add(4 * i), 4, 0);
    }
    top
}

/// The registers the capture handler's stub leaves when it returns (see
/// `BS_FAULT_STUB` in `recorder/src/mpu_trap.c`): r0 = its stack top - 32,
/// r1-r3 and r12 zero, and the flags of its last `cmp` (Z and C set).
fn capture_exit_registers(cpu: &mut Cpu, top: u32) {
    cpu.r[0] = top.wrapping_sub(32);
    for i in [1, 2, 3, 12] {
        cpu.r[i] = 0;
    }
    cpu.flags = backstep_emu::alu::Flags::from_bits(0x6000_0000 | (cpu.xpsr() & (1 << 27)));
}

impl Bus for ReplayBus<'_> {
    fn read(&mut self, address: u32, size: u8) -> Result<u32, Fault> {
        let fault = Fault {
            address,
            size,
            write: false,
        };
        let actual = (self.pc, address, size);
        let peripheral = PERIPHERALS.contains(&address);
        if !peripheral || !self.trapped {
            if PPB.contains(&address) || peripheral {
                self.error = Some(ReplayError::UnrecordedLoad {
                    instruction: self.instruction,
                    actual,
                });
            }
            return Err(fault);
        }
        self.accesses += 1;
        match self.events.get(self.next) {
            Some(e) if (e.pc, e.address, e.size) == actual => {
                self.next += 1;
                Ok(e.value)
            }
            Some(e) => {
                self.error = Some(ReplayError::LoadMismatch {
                    instruction: self.instruction,
                    index: self.next,
                    recorded: *e,
                    actual,
                });
                Err(fault)
            }
            None => {
                self.error = Some(ReplayError::ExtraLoad {
                    instruction: self.instruction,
                    actual,
                });
                Err(fault)
            }
        }
    }

    fn write(&mut self, address: u32, size: u8, _value: u32) -> Result<(), Fault> {
        if PERIPHERALS.contains(&address) {
            // Stores are not recorded (replay does not need their effect);
            // a trapped one still ran the capture handler on the chip.
            self.stores += 1;
            self.accesses += u32::from(self.trapped);
            Ok(())
        } else if PPB.contains(&address) {
            Ok(())
        } else {
            Err(Fault {
                address,
                size,
                write: true,
            })
        }
    }
}

/// Write the CPU state in the format of `qemu-system-arm -d cpu` (with
/// `fpu`, of `-d cpu,fpu`: S0-S31, D0-D15 and FPSCR too).
///
/// # Errors
/// Errors of the writer.
pub fn write_state(cpu: &Cpu, fpu: bool, out: &mut dyn Write) -> std::io::Result<()> {
    let r = &cpu.r;
    for row in 0..4 {
        let b = row * 4;
        writeln!(
            out,
            "R{:02}={:08x} R{:02}={:08x} R{:02}={:08x} R{:02}={:08x}",
            b,
            r[b],
            b + 1,
            r[b + 1],
            b + 2,
            r[b + 2],
            b + 3,
            r[b + 3]
        )?;
    }
    writeln!(out, "XPSR={:08x}", cpu.xpsr())?;
    if fpu {
        for i in 0..16 {
            let (lo, hi) = (cpu.s[2 * i], cpu.s[2 * i + 1]);
            writeln!(
                out,
                "s{:02}={lo:08x} s{:02}={hi:08x} d{i:02}={:016x}",
                2 * i,
                2 * i + 1,
                (u64::from(hi) << 32) | u64::from(lo)
            )?;
        }
        writeln!(out, "FPSCR: {:08x}", cpu.fpscr)?;
    }
    Ok(())
}

/// Build the machine at the start of the recording.
fn start(elf: &Elf, rec: &Recording, target: &Target) -> Result<Cpu, ReplayError> {
    let mut memory = Memory::new();
    for &(base, size, writable) in target.memory {
        memory.add_region(base, size, writable);
    }
    elf.load_into(&mut memory)
        .map_err(|e| ReplayError::Elf(e.to_string()))?;
    if let Some((flash, size)) = target.boot_alias {
        let image: Vec<u8> = (0..size)
            .map(|i| memory.read(flash + i, 1).unwrap_or(0) as u8)
            .collect();
        memory
            .load(0, &image)
            .map_err(|_| ReplayError::Elf("boot alias".into()))?;
    }
    let image: Vec<u8> = (0..rec.image_len)
        .map(|i| memory.read(rec.image_start.wrapping_add(i), 1).unwrap_or(0) as u8)
        .collect();
    let image_crc = crc32(&image);
    if image_crc != rec.image_crc32 {
        return Err(ReplayError::FirmwareMismatch {
            recorded: rec.image_crc32,
            elf: image_crc,
        });
    }
    for region in &rec.snapshot.regions {
        memory
            .load(region.address, &region.bytes)
            .map_err(|_| ReplayError::Elf(format!("snapshot region {:#010x}", region.address)))?;
    }
    // Recordings before format version 3 do not hold the floating-point
    // registers: with the FPU enabled, replay could not start from the
    // recorded state, so it refuses instead of guessing.
    if rec.snapshot.scs.cpacr & (3 << 20) != 0 && !rec.fp_start.valid {
        return Err(ReplayError::Stopped {
            instruction: 0,
            stop: Stop::Unsupported {
                pc: rec.snapshot.cpu.pc & !1,
                what: "recording made with the FPU enabled (CPACR) but without the \
                       floating-point registers (recorded before format version 3)",
            },
        });
    }
    let mut cpu = Cpu::new(memory);
    cpu.scs = Scs::new(target.num_irqs);
    cpu.priority_mask = target.priority_mask;
    restore(&mut cpu, rec);
    Ok(cpu)
}

/// Registers and exception state from the snapshot. The recording starts
/// at the return from `bs_snapshot_take` (r0 = 0, scratch registers 0,
/// flags as at the call: see `recorder/src/snapshot.c`).
fn restore(cpu: &mut Cpu, rec: &Recording) {
    let s = &rec.snapshot.cpu;
    cpu.r[..13].copy_from_slice(&s.r);
    cpu.r[14] = s.lr;
    cpu.r[15] = s.pc & !1;
    cpu.control = s.control & 7;
    cpu.ipsr = s.xpsr & 0x1FF;
    let psp_active = cpu.ipsr == 0 && cpu.control & 2 != 0;
    cpu.r[13] = s.sp;
    cpu.sp_banked = if psp_active { s.msp } else { s.psp };
    cpu.flags = backstep_emu::alu::Flags::from_bits(s.xpsr);
    cpu.ge = ((s.xpsr >> 16) & 0xF) as u8;
    cpu.primask = s.primask & 1 != 0;
    cpu.basepri = (s.basepri as u8) & cpu.priority_mask;
    cpu.faultmask = s.faultmask & 1 != 0;
    cpu.thumb = true;
    cpu.it = 0;

    let scs = &rec.snapshot.scs;
    let mask = cpu.priority_mask;
    cpu.scs.vtor = scs.vtor & 0xFFFF_FF80;
    cpu.scs.prigroup = (scs.aircr >> 8) & 7;
    cpu.scs.ccr = scs.ccr;
    cpu.scs.shcsr_enable = scs.shcsr & (7 << 16);
    for n in 4..16 {
        let byte = (scs.shpr[(n - 4) / 4] >> (8 * ((n - 4) % 4))) as u8;
        cpu.scs.priority[n] = byte & mask;
    }
    for irq in 0..cpu.scs.num_irqs {
        let n = 16 + irq;
        cpu.scs.enabled[n] = scs.nvic_iser[irq / 32] & (1 << (irq % 32)) != 0;
        cpu.scs.priority[n] = scs.nvic_ipr.get(irq).copied().unwrap_or(0) & mask;
    }
    cpu.scs.cpacr = scs.cpacr & vfp::CPACR_WRITABLE;
    let fp = &rec.fp_start;
    if fp.valid {
        cpu.s = fp.s;
        cpu.fpscr = fp.fpscr & backstep_emu::fpu::fpscr::WRITABLE;
        cpu.scs.fpccr = fp.fpccr & vfp::fpccr::WRITABLE_ALL;
        cpu.scs.fpcar = fp.fpcar & !7;
        cpu.scs.fpdscr = fp.fpdscr & vfp::FPDSCR_WRITABLE;
    }
}

/// `bs_stack_hash` of the stack above `sp` (at most 128 bytes, not past
/// `top`), as the recorder computes it for an interrupt key.
fn stack_hash(cpu: &Cpu, sp: u32, top: u32) -> u32 {
    let words = (top.saturating_sub(sp) / 4).min(32);
    (0..words).fold(0x811C_9DC5u32, |h, i| {
        let w = cpu.memory.peek(sp.wrapping_add(4 * i), 4).unwrap_or(0);
        (h ^ w).wrapping_mul(0x0100_0193)
    })
}

/// Whether the core is at the position where interrupt `e` arrived:
/// same resume PC, registers, xPSR, peripheral-read count and stack.
fn at_position(cpu: &Cpu, e: &IrqEvent, loads: usize, stack_top: u32) -> bool {
    cpu.r[15] == e.pc
        && cpu.r[..13] == e.r
        && cpu.r[13] == e.sp
        && cpu.r[14] == e.lr
        && cpu.xpsr() == e.xpsr
        && loads == e.mmio_index as usize
        && stack_hash(cpu, e.sp, stack_top) == e.stack_hash
}

/// The registers compared at the end: those the recorder captures (r4-r11,
/// SP, LR, flags and GE, and the special registers).
fn compare_end(cpu: &Cpu, end: &CpuState) -> Result<(), ReplayError> {
    let mismatch = |what: &str, recorded: u32, replayed: u32| {
        if recorded == replayed {
            Ok(())
        } else {
            Err(ReplayError::EndMismatch {
                what: what.to_string(),
                recorded,
                replayed,
            })
        }
    };
    for i in 4..12 {
        mismatch(&format!("r{i}"), end.r[i], cpu.r[i])?;
    }
    mismatch("sp", end.sp, cpu.r[13])?;
    mismatch("lr", end.lr, cpu.r[14])?;
    mismatch(
        "APSR (flags, GE)",
        end.xpsr & 0xF80F_0000,
        cpu.xpsr() & 0xF80F_0000,
    )?;
    mismatch("msp", end.msp, cpu.msp())?;
    mismatch("psp", end.psp, cpu.psp())?;
    mismatch("primask", end.primask & 1, u32::from(cpu.primask))?;
    mismatch("basepri", end.basepri & 0xFF, u32::from(cpu.basepri))?;
    mismatch("faultmask", end.faultmask & 1, u32::from(cpu.faultmask))?;
    mismatch("control", end.control & 3, cpu.control & 3)
}

/// The floating-point registers and context at the end.
fn compare_fp_end(cpu: &Cpu, fp: &FpState) -> Result<(), ReplayError> {
    let mismatch = |what: String, recorded: u32, replayed: u32| {
        if recorded == replayed {
            Ok(())
        } else {
            Err(ReplayError::EndMismatch {
                what,
                recorded,
                replayed,
            })
        }
    };
    for (i, (r, c)) in fp.s.iter().zip(cpu.s).enumerate() {
        mismatch(format!("s{i}"), *r, c)?;
    }
    mismatch("fpscr".into(), fp.fpscr, cpu.fpscr)?;
    mismatch("fpccr".into(), fp.fpccr, cpu.scs.fpccr)?;
    mismatch("fpcar".into(), fp.fpcar & !7, cpu.scs.fpcar)?;
    mismatch("fpdscr".into(), fp.fpdscr, cpu.scs.fpdscr)
}

/// At the end point: everything recorded was replayed, and the registers
/// and RAM checksum are the recorded ones. Returns the RAM checksum.
fn check_end(cpu: &Cpu, rec: &Recording, loads: usize, irqs: usize) -> Result<u32, ReplayError> {
    if irqs != rec.irqs.len() {
        return Err(ReplayError::InterruptsLeft {
            delivered: irqs,
            recorded: rec.irqs.len(),
            next: rec.irqs[irqs],
        });
    }
    if loads != rec.events.len() {
        return Err(ReplayError::LoadsLeft {
            replayed: loads,
            recorded: rec.events.len(),
        });
    }
    compare_end(cpu, &rec.end.cpu)?;
    // The end capture's first floating-point instruction may preserve
    // floating-point state lazily (into RAM) and create a new context
    // before the registers are read; do the same to a copy.
    let mut after = cpu.clone();
    let fp = &rec.fp_end;
    let accessible = after.execute_fp_check(&mut NoBus).is_ok();
    if accessible != fp.valid {
        return Err(ReplayError::EndMismatch {
            what: "access to the FPU (CPACR)".into(),
            recorded: u32::from(fp.valid),
            replayed: u32::from(accessible),
        });
    }
    if fp.valid {
        compare_fp_end(&after, fp)?;
    }
    let cpu = &after;
    let mut crc = 0;
    for &(address, len) in &rec.end.regions {
        let bytes: Vec<u8> = (0..len)
            .map(|i| cpu.memory.peek(address.wrapping_add(i), 1).unwrap_or(0) as u8)
            .collect();
        crc = update(crc, &bytes);
    }
    if crc != rec.end.ram_crc32 {
        return Err(ReplayError::EndMismatch {
            what: "the RAM checksum".into(),
            recorded: rec.end.ram_crc32,
            replayed: crc,
        });
    }
    Ok(crc)
}

/// The replayed machine at one position of the recording: the state
/// before instruction `n`, with the interrupts due there delivered. Cheap
/// to clone (memory pages are shared), so checkpoints are clones.
#[derive(Debug, Clone)]
pub(crate) struct Machine {
    pub(crate) cpu: Cpu,
    /// Instructions executed: the position.
    pub(crate) n: u64,
    /// Recorded loads used.
    pub(crate) loads: usize,
    /// Recorded interrupts delivered.
    pub(crate) irqs: usize,
    /// Peripheral stores (not recorded).
    pub(crate) stores: u64,
    /// If the last instruction made a trapped peripheral access: the
    /// capture handler's stack top (see [`leave_trap_frame`]).
    pub(crate) trap_top: Option<u32>,
}

/// Where the recorded interrupts are delivered.
#[derive(Debug, Clone, Default)]
pub(crate) struct Plan {
    /// The positions of the interrupts placed so far, in order (equal when
    /// one preempted another before its first instruction).
    pub(crate) positions: Vec<u64>,
    /// The next interrupt is not placed before this position: its key
    /// matched earlier, but there the core could not take it, or the
    /// replay diverged after it (lookahead).
    pub(crate) not_before: u64,
    /// When going back: the next interrupt must be placed by this
    /// position, or this attempt has diverged too.
    pub(crate) search_until: Option<u64>,
    /// Chains of interrupts delivered at one position outermost first:
    /// first index to last (see [`Engine::deliver`]).
    pub(crate) chains: BTreeMap<usize, usize>,
    /// The machines just before the most recent placements (at most
    /// [`RECENT`]), to go back to: (interrupt index, machine).
    pub(crate) recent: VecDeque<(usize, Machine)>,
}

/// The longest chain of interrupts preempting each other before their
/// stubs ran.
const MAX_CHAIN: usize = 8;
/// How many recent interrupt placements replay can go back to.
const RECENT: usize = 64;
/// How far (instructions) replay looks for a later match of a key after
/// going back, at least.
const MIN_SEARCH: u64 = 2_000_000;
/// Instructions replay may spend on going back before it reports a
/// divergence: this many, or four times the first attempt, if more.
const RETRY_BUDGET: u64 = 50_000_000;

/// Runs a recording on the emulator.
pub(crate) struct Engine<'r> {
    pub(crate) rec: &'r Recording,
    pub(crate) target: Target,
    pub(crate) stop_pc: u32,
    exceptions: usize,
}

impl<'r> Engine<'r> {
    /// The engine and the machine at position 0, before delivery.
    pub(crate) fn new(elf: &Elf, rec: &'r Recording) -> Result<(Self, Machine), ReplayError> {
        let target =
            target::by_id(rec.target_id).ok_or(ReplayError::UnknownTarget(rec.target_id))?;
        let stop_pc = elf
            .symbol("bs_snapshot_take")
            .ok_or_else(|| ReplayError::Elf("no symbol bs_snapshot_take".into()))?
            & !1;
        let cpu = start(elf, rec, &target)?;
        let exceptions = 16 + target.num_irqs;
        let engine = Engine {
            rec,
            target,
            stop_pc,
            exceptions,
        };
        let machine = Machine {
            cpu,
            n: 0,
            loads: 0,
            irqs: 0,
            stores: 0,
            trap_top: None,
        };
        Ok((engine, machine))
    }

    fn bus(&self, m: &Machine) -> ReplayBus<'r> {
        ReplayBus {
            events: &self.rec.events,
            next: m.loads,
            pc: m.cpu.r[15],
            instruction: m.n,
            stores: m.stores,
            accesses: 0,
            trapped: true,
            error: None,
        }
    }

    /// Whether `m` is at the end of the recording.
    pub(crate) fn at_end(&self, m: &Machine) -> bool {
        m.cpu.r[15] == self.stop_pc
    }

    /// Deliver the interrupts due at `m`'s position. Interrupts the plan
    /// has placed are delivered where it says; with `discover`, the next
    /// one is delivered where its key matches (not before
    /// `plan.not_before`) and placed in the plan.
    pub(crate) fn deliver(
        &self,
        m: &mut Machine,
        plan: &mut Plan,
        discover: bool,
    ) -> Result<(), ReplayError> {
        while m.irqs < self.rec.irqs.len() {
            let k = m.irqs;
            let placed = k < plan.positions.len();
            // `k..=last` are delivered here, `last` first: an interrupt
            // that preempted another before its stub's first instruction
            // was logged before it (ADR 0004), so such a chain is logged
            // innermost first.
            let last = if placed {
                if plan.positions[k] != m.n {
                    break;
                }
                plan.chains.get(&k).copied().unwrap_or(k)
            } else if !discover || m.n < plan.not_before {
                break;
            } else if self.matches(m, k) {
                k
            } else if let Some(last) = self.chain_matching(m, k) {
                last
            } else {
                break;
            };
            let before = (!placed).then(|| m.clone());
            if last == k {
                self.take(m, k)?;
            } else {
                // Deliver the chain outermost first; each inner key must
                // then match at the stub entry of the one it preempted.
                let mut t = m.clone();
                for i in (k..=last).rev() {
                    if i + 1 == last && !self.matches(&t, i) {
                        if let Some(top) = t.trap_top {
                            // On the chip the outer interrupt tail-chained
                            // from the capture handler, so its stub began
                            // with the capture's scratch registers.
                            capture_exit_registers(&mut t.cpu, top);
                        }
                    }
                    if !placed && i != last && !self.matches(&t, i) {
                        return Err(ReplayError::InterruptNotTaken {
                            instruction: m.n,
                            index: k,
                            exception: self.rec.irqs[k].exception,
                        });
                    }
                    self.take(&mut t, i)?;
                }
                *m = t;
            }
            if let Some(before) = before {
                plan.positions
                    .extend(std::iter::repeat_n(m.n, last + 1 - k));
                if last > k {
                    plan.chains.insert(k, last);
                }
                plan.not_before = 0;
                plan.search_until = None;
                if plan.recent.len() == RECENT {
                    plan.recent.pop_front();
                }
                plan.recent.push_back((k, before));
            }
            m.irqs = last + 1;
        }
        Ok(())
    }

    /// Whether the state is at the recorded position of interrupt `i`.
    fn matches(&self, m: &Machine, i: usize) -> bool {
        at_position(&m.cpu, &self.rec.irqs[i], m.loads, self.rec.irq_stack_top)
    }

    /// The outermost interrupt of a chain starting at `k` (each one
    /// preempting the next before its stub ran: its xPSR names the next
    /// one's exception and it has the same read count) whose key matches
    /// here, if any.
    fn chain_matching(&self, m: &Machine, k: usize) -> Option<usize> {
        let irqs = &self.rec.irqs;
        let mut j = k;
        while j + 1 < irqs.len()
            && j - k < MAX_CHAIN
            && irqs[j].xpsr & 0x1FF == irqs[j + 1].exception
            && irqs[j].mmio_index == irqs[j + 1].mmio_index
        {
            j += 1;
            if self.matches(m, j) {
                return Some(j);
            }
        }
        None
    }

    /// Take recorded interrupt `i` now.
    fn take(&self, m: &mut Machine, i: usize) -> Result<(), ReplayError> {
        let event = &self.rec.irqs[i];
        let exception = event.exception;
        let number = exception as usize;
        let stopped = |stop| ReplayError::Stopped {
            instruction: m.n,
            stop,
        };
        let not_taken = ReplayError::InterruptNotTaken {
            instruction: m.n,
            index: i,
            exception,
        };
        if number < 2 || number >= self.exceptions {
            return Err(not_taken);
        }
        let mut bus = self.bus(m);
        // Right after an exception return, the interrupt either tail-chained
        // (it was pending at the return) or arrived just after it. The
        // integer state is the same either way, the floating-point state
        // not; the FPCCR the stub recorded tells them apart (ADR 0012).
        if let Some(fpccr) = event.fpccr {
            let mut chained = m.cpu.clone();
            if chained
                .tail_chain_after_return(&mut bus, number)
                .map_err(stopped)?
                && chained.scs.fpccr == fpccr
            {
                let mut fresh = m.cpu.clone();
                let fresh_ok = fresh.interrupt_now(&mut bus, number).map_err(stopped)?
                    && fresh.scs.fpccr == fpccr;
                if !fresh_ok {
                    m.cpu = chained;
                    return Ok(());
                }
            }
        }
        if !m.cpu.interrupt_now(&mut bus, number).map_err(stopped)? {
            return Err(not_taken);
        }
        if let Some(fpccr) = event.fpccr {
            if m.cpu.scs.fpccr != fpccr {
                return Err(ReplayError::FpccrMismatch {
                    instruction: m.n,
                    index: i,
                    recorded: fpccr,
                    replayed: m.cpu.scs.fpccr,
                });
            }
        }
        Ok(())
    }

    /// Execute the instruction at `m`'s position.
    pub(crate) fn execute(&self, m: &mut Machine) -> Result<(), ReplayError> {
        let mut bus = self.bus(m);
        // Accesses at priority -1 or -2 (HardFault, NMI) bypass the MPU, so
        // the capture did not see them.
        bus.trapped = m.cpu.execution_priority() >= 0;
        let frame_on_psp = m.cpu.ipsr == 0 && m.cpu.control & 2 != 0;
        let result = m.cpu.step(&mut bus);
        if let Some(e) = bus.error.take() {
            return Err(e);
        }
        result.map_err(|stop| ReplayError::Stopped {
            instruction: m.n,
            stop,
        })?;
        m.loads = bus.next;
        m.stores = bus.stores;
        m.trap_top = (bus.accesses > 0).then(|| leave_trap_frame(&mut m.cpu, frame_on_psp));
        m.n += 1;
        Ok(())
    }

    /// Run from `m` to the end of the recording, delivering interrupts as
    /// `plan` says (discovering the rest), calling `visit` at every
    /// position.
    pub(crate) fn run(
        &self,
        m: &mut Machine,
        plan: &mut Plan,
        max_instructions: u64,
        visit: &mut dyn FnMut(&Machine) -> Result<(), ReplayError>,
    ) -> Result<(), ReplayError> {
        loop {
            visit(m)?;
            self.deliver(m, plan, true)?;
            if self.at_end(m) {
                return Ok(());
            }
            if plan.search_until.is_some_and(|until| m.n >= until) {
                // The interrupt sought after going back is not found.
                return Err(ReplayError::InterruptsLeft {
                    delivered: m.irqs,
                    recorded: self.rec.irqs.len(),
                    next: self.rec.irqs[m.irqs],
                });
            }
            if m.n >= max_instructions {
                return Err(ReplayError::InstructionLimit(m.n));
            }
            self.execute(m)?;
        }
    }

    /// Replay the whole recording and check it. When the replay diverges,
    /// go back to the most recent interrupt placed and deliver it where
    /// its key matches next instead (ADR 0004: a key may not tell loop
    /// iterations apart), and so on back, within [`RECENT`] placements and
    /// a budget. Returns the plan of the replay that reproduced the
    /// recording, its end machine and the RAM checksum; otherwise the
    /// divergence of the first attempt.
    pub(crate) fn verify(
        &self,
        start: Machine,
        max_instructions: u64,
    ) -> Result<Verified, ReplayError> {
        let mut plan = Plan::default();
        let mut m = start;
        let mut first_error: Option<ReplayError> = None;
        let mut retries = 0;
        let mut budget = RETRY_BUDGET;
        let mut spent = 0u64;
        let mut search = MIN_SEARCH;
        loop {
            let from = m.n;
            let result = self
                .run(&mut m, &mut plan, max_instructions, &mut |_| Ok(()))
                .and_then(|()| check_end(&m.cpu, self.rec, m.loads, m.irqs));
            let err = match result {
                Ok(crc) => {
                    plan.recent.clear();
                    return Ok(Verified {
                        plan,
                        end: m,
                        ram_crc32: crc,
                        retries,
                    });
                }
                Err(e) => e,
            };
            if first_error.is_none() {
                budget = budget.max(4 * m.n);
                // Search for a later match at most twice as far as the
                // longest gap between interrupts seen so far.
                let gap = plan
                    .positions
                    .windows(2)
                    .map(|w| w[1] - w[0])
                    .max()
                    .unwrap_or(0);
                search = search.max(2 * gap);
            } else {
                spent += m.n.saturating_sub(from);
            }
            let first = first_error.get_or_insert_with(|| err.clone()).clone();
            if matches!(err, ReplayError::InstructionLimit(_)) || spent > budget {
                return Err(first);
            }
            retries += 1;
            if let ReplayError::InterruptNotTaken { instruction, .. } = err {
                // The key matched where the core cannot take the interrupt:
                // not its position. Nothing changed; look further on.
                plan.not_before = instruction + 1;
                plan.search_until = None;
                continue;
            }
            // Go back to the most recent placement and deliver that
            // interrupt where its key matches next instead: ADR 0004 found
            // keys that loop iterations share.
            let Some((index, before)) = plan.recent.pop_back() else {
                return Err(first);
            };
            plan.positions.truncate(index);
            plan.chains.retain(|&k, _| k < index);
            plan.not_before = before.n + 1;
            plan.search_until = Some(before.n + search);
            m = before;
        }
    }
}

/// A replay that reproduced its recording.
pub(crate) struct Verified {
    pub(crate) plan: Plan,
    pub(crate) end: Machine,
    pub(crate) ram_crc32: u32,
    /// Interrupts delivered later than their first key match.
    pub(crate) retries: u32,
}

/// Replay `rec` on the firmware `elf`.
///
/// The recording ends at the entry of `bs_snapshot_take` called from
/// `bs_record_end`: the first time the emulator reaches that symbol.
///
/// # Errors
/// [`ReplayError`] on any difference from the recording.
pub fn replay(elf: &Elf, rec: &Recording, mut opts: Options<'_>) -> Result<Report, ReplayError> {
    let (engine, start) = Engine::new(elf, rec)?;
    let verified = engine.verify(start.clone(), opts.max_instructions)?;
    if let Some(out) = opts.trace.as_deref_mut() {
        // The trace of the replay that reproduced the recording; with the
        // floating-point registers when the recording has them.
        let fpu = rec.fp_start.valid;
        let mut m = start;
        let mut plan = verified.plan.clone();
        loop {
            engine.deliver(&mut m, &mut plan, false)?;
            write_state(&m.cpu, fpu, out).map_err(|e| ReplayError::Elf(format!("trace: {e}")))?;
            if engine.at_end(&m) {
                break;
            }
            engine.execute(&mut m)?;
        }
    }
    let end = &verified.end;
    Ok(Report {
        target: engine.target,
        instructions: end.n,
        loads: end.loads,
        stores: end.stores,
        interrupts: end.irqs,
        ram_crc32: verified.ram_crc32,
        retries: verified.retries,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cpu() -> Cpu {
        let mut memory = Memory::new();
        memory.add_region(0x2000_0000, 0x1000, true);
        let mut cpu = Cpu::new(memory);
        for (i, r) in cpu.r.iter_mut().enumerate() {
            *r = 0x100 + i as u32;
        }
        cpu.r[15] = 0x0000_0222;
        for i in 0..0x400 {
            cpu.memory.write(0x2000_0000 + 4 * i, 4, 0xDEAD_BEEF);
        }
        cpu
    }

    fn word(cpu: &Cpu, address: u32) -> u32 {
        cpu.memory.read(address, 4).unwrap()
    }

    #[test]
    fn the_trap_frame_is_the_chips_exception_frame() {
        // SP 8-byte aligned: the frame sits right below it.
        let mut c = cpu();
        c.r[13] = 0x2000_0F00;
        let xpsr = c.xpsr();
        leave_trap_frame(&mut c, false);
        let f = 0x2000_0F00 - 32;
        let want = [0x100, 0x101, 0x102, 0x103, 0x10C, 0x10E, 0x222, xpsr];
        for (i, w) in want.iter().enumerate() {
            assert_eq!(word(&c, f + 4 * i as u32), *w, "frame word {i}");
        }
        // The handler's dead stack below the frame is zero, and nothing
        // below that or above SP changed.
        for a in (f - TRAP_STACK_BYTES..f).step_by(4) {
            assert_eq!(word(&c, a), 0, "{a:#x}");
        }
        assert_eq!(word(&c, f - TRAP_STACK_BYTES - 4), 0xDEAD_BEEF);
        assert_eq!(word(&c, 0x2000_0F00), 0xDEAD_BEEF);
        assert_eq!(c.r[13], 0x2000_0F00, "SP is not changed");
    }

    #[test]
    fn a_misaligned_sp_gets_the_padding_word_and_bit_9() {
        let mut c = cpu();
        c.r[13] = 0x2000_0F04;
        leave_trap_frame(&mut c, false);
        let f = 0x2000_0F04 - 36;
        assert_eq!(word(&c, f + 24), 0x222);
        assert_eq!(word(&c, f + 28) & (1 << 9), 1 << 9);
        assert_eq!(
            word(&c, f + 32),
            0xDEAD_BEEF,
            "the padding word keeps its value"
        );
        // Without STKALIGN there is no realignment.
        let mut c = cpu();
        c.scs.ccr = 0;
        c.r[13] = 0x2000_0F04;
        leave_trap_frame(&mut c, false);
        assert_eq!(word(&c, 0x2000_0F04 - 32 + 24), 0x222);
        assert_eq!(word(&c, 0x2000_0F04 - 32 + 28) & (1 << 9), 0);
    }

    #[test]
    fn on_the_process_stack_the_handler_zeroes_below_msp() {
        let mut c = cpu();
        c.r[13] = 0x2000_0800; // PSP, the current SP in thread mode
        c.control = 2;
        c.sp_banked = 0x2000_0F00; // MSP
        leave_trap_frame(&mut c, true);
        assert_eq!(word(&c, 0x2000_0800 - 32 + 24), 0x222);
        assert_eq!(word(&c, 0x2000_0F00 - 4), 0);
        assert_eq!(word(&c, 0x2000_0F00 - TRAP_STACK_BYTES), 0);
        assert_eq!(word(&c, 0x2000_0F00 - TRAP_STACK_BYTES - 4), 0xDEAD_BEEF);
        assert_eq!(word(&c, 0x2000_0800 - 36), 0xDEAD_BEEF);
    }
}
