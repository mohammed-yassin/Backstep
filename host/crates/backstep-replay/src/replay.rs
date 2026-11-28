//! Re-run a recording on the emulator and check it.

use std::fmt;
use std::io::Write;

use backstep_core::crc32::{crc32, update};
use backstep_emu::elf::Elf;
use backstep_emu::memory::{Bus, Fault};
use backstep_emu::{Cpu, Memory, Scs, Stop};

use crate::recording::{CpuState, IrqEvent, MmioEvent, Recording};
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
fn leave_trap_frame(cpu: &mut Cpu, frame_on_psp: bool) {
    let sp = cpu.r[13];
    let realign = cpu.scs.ccr & (1 << 9) != 0 && sp & 4 != 0;
    let frame = sp.wrapping_sub(0x20) & !(u32::from(realign) << 2);
    let xpsr = cpu.xpsr() | (u32::from(realign) << 9);
    let words = [
        cpu.r[0], cpu.r[1], cpu.r[2], cpu.r[3], cpu.r[12], cpu.r[14], cpu.r[15], xpsr,
    ];
    for (i, w) in words.iter().enumerate() {
        cpu.memory.write(frame.wrapping_add(4 * i as u32), 4, *w);
    }
    let top = if frame_on_psp { cpu.msp() } else { frame };
    for i in 0..TRAP_STACK_BYTES / 4 {
        cpu.memory
            .write(top.wrapping_sub(TRAP_STACK_BYTES).wrapping_add(4 * i), 4, 0);
    }
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

/// `bs_stack_hash` of the stack above `sp` (at most 128 bytes, not past
/// `top`), as the recorder computes it for an interrupt key.
fn stack_hash(cpu: &Cpu, sp: u32, top: u32) -> u32 {
    let words = (top.saturating_sub(sp) / 4).min(32);
    (0..words).fold(0x811C_9DC5u32, |h, i| {
        let w = cpu.memory.read(sp.wrapping_add(4 * i), 4).unwrap_or(0);
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

/// Deliver every recorded interrupt whose position the core is at (more
/// than one when an interrupt preempted another's first instruction).
fn deliver_interrupts(
    cpu: &mut Cpu,
    bus: &mut ReplayBus<'_>,
    rec: &Recording,
    next_irq: &mut usize,
    exceptions: usize,
) -> Result<(), ReplayError> {
    while let Some(e) = rec.irqs.get(*next_irq) {
        if !at_position(cpu, e, bus.next, rec.irq_stack_top) {
            break;
        }
        let number = e.exception as usize;
        let taken = number >= 2
            && number < exceptions
            && cpu
                .interrupt_now(bus, number)
                .map_err(|stop| ReplayError::Stopped {
                    instruction: bus.instruction,
                    stop,
                })?;
        if !taken {
            return Err(ReplayError::InterruptNotTaken {
                instruction: bus.instruction,
                index: *next_irq,
                exception: e.exception,
            });
        }
        *next_irq += 1;
    }
    Ok(())
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
    Ok(crc)
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
        accesses: 0,
        trapped: true,
        error: None,
    };
    let mut n = 0u64;
    let mut next_irq = 0usize;
    let exceptions = 16 + target.num_irqs;
    loop {
        bus.instruction = n;
        deliver_interrupts(&mut cpu, &mut bus, rec, &mut next_irq, exceptions)?;
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
        bus.accesses = 0;
        // Accesses at priority -1 or -2 (HardFault, NMI) bypass the MPU, so
        // the capture did not see them.
        bus.trapped = cpu.execution_priority() >= 0;
        let frame_on_psp = cpu.ipsr == 0 && cpu.control & 2 != 0;
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
        if bus.accesses > 0 {
            leave_trap_frame(&mut cpu, frame_on_psp);
        }
        n += 1;
    }
    let crc = check_end(&cpu, rec, bus.next, next_irq)?;
    Ok(Report {
        target,
        instructions: n,
        loads: bus.next,
        stores: bus.stores,
        interrupts: next_irq,
        ram_crc32: crc,
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
