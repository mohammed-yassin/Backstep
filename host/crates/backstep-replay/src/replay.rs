//! Re-run a recording on the emulator and check it.

use std::fmt;
use std::io::Write;

use backstep_core::crc32::{crc32, update};
use backstep_emu::elf::Elf;
use backstep_emu::memory::{Bus, Fault};
use backstep_emu::{Cpu, Memory, Scs, Stop};

use crate::recording::{CpuState, MmioEvent, Recording};
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
    /// CRC-32 of the end RAM regions (equal to the recording's).
    pub ram_crc32: u32,
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
    /// A load from an address the recorder does not capture (PPB, e.g.
    /// SysTick or DWT), whose value replay cannot know.
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
    /// The end point was reached with recorded loads left over.
    LoadsLeft {
        /// Loads replayed.
        replayed: usize,
        /// Loads recorded.
        recorded: usize,
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
                "at instruction {instruction}: {} is not recorded (not a captured peripheral)",
                access(*actual)
            ),
            ReplayError::Stopped { instruction, stop } => {
                write!(f, "the emulator stopped at instruction {instruction}: {stop:?}")
            }
            ReplayError::InstructionLimit(n) => {
                write!(f, "the end of the recording was not reached in {n} instructions")
            }
            ReplayError::LoadsLeft { replayed, recorded } => write!(
                f,
                "diverged: the end was reached after {replayed} of {recorded} recorded loads"
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
    error: Option<ReplayError>,
}

impl Bus for ReplayBus<'_> {
    fn read(&mut self, address: u32, size: u8) -> Result<u32, Fault> {
        let fault = Fault {
            address,
            size,
            write: false,
        };
        let actual = (self.pc, address, size);
        if !PERIPHERALS.contains(&address) {
            if PPB.contains(&address) {
                self.error = Some(ReplayError::UnrecordedLoad {
                    instruction: self.instruction,
                    actual,
                });
            }
            return Err(fault);
        }
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
        if PERIPHERALS.contains(&address) || PPB.contains(&address) {
            self.stores += 1;
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

/// Write the CPU state in the format of `qemu-system-arm -d cpu`.
///
/// # Errors
/// Errors of the writer.
pub fn write_state(cpu: &Cpu, out: &mut dyn Write) -> std::io::Result<()> {
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
    writeln!(out, "XPSR={:08x}", cpu.xpsr())
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

/// Replay `rec` on the firmware `elf`.
///
/// The recording ends at the entry of `bs_snapshot_take` called from
/// `bs_record_end`: the first time the emulator reaches that symbol.
///
/// # Errors
/// [`ReplayError`] on any difference from the recording.
pub fn replay(elf: &Elf, rec: &Recording, mut opts: Options<'_>) -> Result<Report, ReplayError> {
    let target = target::by_id(rec.target_id).ok_or(ReplayError::UnknownTarget(rec.target_id))?;
    let stop_pc = elf
        .symbol("bs_snapshot_take")
        .ok_or_else(|| ReplayError::Elf("no symbol bs_snapshot_take".into()))?
        & !1;
    let mut cpu = start(elf, rec, &target)?;
    let mut bus = ReplayBus {
        events: &rec.events,
        next: 0,
        pc: 0,
        instruction: 0,
        stores: 0,
        error: None,
    };
    let mut n = 0u64;
    loop {
        if let Some(out) = opts.trace.as_deref_mut() {
            write_state(&cpu, out).map_err(|e| ReplayError::Elf(format!("trace: {e}")))?;
        }
        if cpu.r[15] == stop_pc {
            break;
        }
        if n >= opts.max_instructions {
            return Err(ReplayError::InstructionLimit(n));
        }
        bus.pc = cpu.r[15];
        bus.instruction = n;
        let result = cpu.step(&mut bus);
        if let Some(e) = bus.error.take() {
            return Err(e);
        }
        if let Err(stop) = result {
            return Err(ReplayError::Stopped {
                instruction: n,
                stop,
            });
        }
        n += 1;
    }
    if bus.next != rec.events.len() {
        return Err(ReplayError::LoadsLeft {
            replayed: bus.next,
            recorded: rec.events.len(),
        });
    }
    compare_end(&cpu, &rec.end.cpu)?;
    let mut crc = 0;
    for &(address, len) in &rec.end.regions {
        let bytes: Vec<u8> = (0..len)
            .map(|i| cpu.memory.read(address.wrapping_add(i), 1).unwrap_or(0) as u8)
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
    Ok(Report {
        target,
        instructions: n,
        loads: bus.next,
        stores: bus.stores,
        ram_crc32: crc,
    })
}
