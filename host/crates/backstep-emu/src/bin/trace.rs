//! `backstep-emu-trace <firmware.elf> [max-instructions]`
//!
//! Runs a test firmware image on the emulator with the QEMU `mps2-an386`
//! memory map and prints the register state before every instruction in
//! exactly the format of `qemu-system-arm -d cpu`, so the two traces can be
//! compared line by line (`scripts/emu-difftest.py`). Stops at a
//! semihosting exit (`BKPT 0xAB` with r0 = 0x18). The semihosting calls
//! `SYS_OPEN`, `SYS_WRITE` and `SYS_CLOSE` write files relative to the
//! current directory, as QEMU's `target=native` semihosting does, so a
//! program can dump its memory for comparison.

use std::process::ExitCode;

use backstep_emu::cpu::exceptions::FAULT_CAUSES;
use backstep_emu::elf::Elf;
use backstep_emu::memory::NoBus;
use backstep_emu::{Cpu, Memory, Scs, Stop};

/// Exception event counters on stderr (`STAT <name> <count>`), used by
/// the differential test to measure coverage of the exception model.
fn print_stats(cpu: &Cpu) {
    let s = &cpu.stats;
    for (n, count) in s.entries.iter().enumerate() {
        if *count > 0 {
            eprintln!("STAT entry-{n} {count}");
        }
    }
    for (name, count) in [
        ("nested", s.nested),
        ("tail-chained", s.tail_chained),
        ("escalated", s.escalated),
        ("realigned", s.realigned),
        ("process-stack", s.process_stack),
        ("return-handler", s.returns[0]),
        ("return-thread-msp", s.returns[1]),
        ("return-thread-psp", s.returns[2]),
    ] {
        eprintln!("STAT {name} {count}");
    }
    for ((name, _), count) in FAULT_CAUSES.iter().zip(s.faults) {
        eprintln!("STAT fault-{name} {count}");
    }
}

fn print_state(cpu: &Cpu, out: &mut String) {
    use std::fmt::Write;
    let r = &cpu.r;
    for row in 0..4 {
        let b = row * 4;
        let _ = writeln!(
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
        );
    }
    let _ = writeln!(out, "XPSR={:08x}", cpu.xpsr());
}

/// Open files of the semihosting calls (handle = index + 1).
#[derive(Default)]
struct Semihosting {
    files: Vec<Option<std::fs::File>>,
}

impl Semihosting {
    fn word(cpu: &Cpu, address: u32) -> u32 {
        cpu.memory.read(address, 4).unwrap_or(0)
    }

    fn bytes(cpu: &Cpu, address: u32, len: u32) -> Vec<u8> {
        (0..len)
            .map(|i| {
                cpu.memory
                    .read(address.wrapping_add(i), 1)
                    .and_then(|b| u8::try_from(b).ok())
                    .unwrap_or(0)
            })
            .collect()
    }

    /// Handle `BKPT 0xAB` with operation r0 and parameter block r1; the
    /// result goes to r0. `None` for operations not supported.
    fn call(&mut self, cpu: &mut Cpu) -> Option<()> {
        use std::io::Write;
        let block = cpu.r[1];
        let arg = |i: u32| Self::word(cpu, block + 4 * i);
        let result = match cpu.r[0] {
            0x01 => {
                // SYS_OPEN: name, mode (4-11: write), name length.
                let (name, mode, len) = (arg(0), arg(1), arg(2));
                let bytes = Self::bytes(cpu, name, len);
                let path = String::from_utf8_lossy(&bytes).into_owned();
                let file = if mode >= 4 {
                    std::fs::File::create(&path).ok()?
                } else {
                    std::fs::File::open(&path).ok()?
                };
                self.files.push(Some(file));
                u32::try_from(self.files.len()).ok()?
            }
            0x05 => {
                // SYS_WRITE: handle, buffer, length; returns bytes not written.
                let (handle, buffer, len) = (arg(0), arg(1), arg(2));
                let bytes = Self::bytes(cpu, buffer, len);
                let file = self
                    .files
                    .get_mut(handle.checked_sub(1)? as usize)?
                    .as_mut()?;
                file.write_all(&bytes).ok()?;
                0
            }
            0x02 => {
                // SYS_CLOSE: handle.
                let handle = arg(0);
                self.files
                    .get_mut(handle.checked_sub(1)? as usize)?
                    .take()?;
                0
            }
            _ => return None,
        };
        cpu.r[0] = result;
        cpu.r[15] += 2; // continue after the BKPT
        Some(())
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let Some(path) = args.get(1) else {
        eprintln!("usage: backstep-emu-trace <firmware.elf> [max-instructions]");
        return ExitCode::from(2);
    };
    let max: u64 = args
        .get(2)
        .and_then(|s| s.parse().ok())
        .unwrap_or(10_000_000);
    let elf = match std::fs::read(path)
        .map_err(|e| e.to_string())
        .and_then(|d| Elf::parse(d).map_err(|e| e.to_string()))
    {
        Ok(e) => e,
        Err(e) => {
            eprintln!("backstep-emu-trace: {path}: {e}");
            return ExitCode::from(2);
        }
    };
    let mut memory = Memory::new();
    memory.add_region(0x0000_0000, 0x0040_0000, true); // ZBT SSRAM1 (code)
    memory.add_region(0x2000_0000, 0x0040_0000, true); // SSRAM2/3 (data)
    if let Err(e) = elf.load_into(&mut memory) {
        eprintln!("backstep-emu-trace: {e}");
        return ExitCode::from(2);
    }
    let mut cpu = Cpu::new(memory);
    cpu.scs = Scs::new(32); // mps2-an386: 32 external interrupts
    if cpu.reset(0).is_err() {
        eprintln!("backstep-emu-trace: no vector table at 0");
        return ExitCode::from(2);
    }
    let mut out = String::new();
    let mut bus = NoBus;
    let mut semihosting = Semihosting::default();
    for _ in 0..max {
        print_state(&cpu, &mut out);
        match cpu.step(&mut bus) {
            Ok(()) => {}
            Err(Stop::Breakpoint { imm: 0xAB, .. })
                if cpu.r[0] != 0x18 && semihosting.call(&mut cpu).is_some() => {}
            Err(Stop::Breakpoint { imm: 0xAB, .. }) if cpu.r[0] == 0x18 => {
                print!("{out}");
                print_stats(&cpu);
                return ExitCode::SUCCESS;
            }
            Err(stop) => {
                print!("{out}");
                print_stats(&cpu);
                eprintln!("backstep-emu-trace: stopped: {stop:?}");
                return ExitCode::from(1);
            }
        }
    }
    print!("{out}");
    eprintln!("backstep-emu-trace: instruction limit reached");
    ExitCode::from(1)
}
