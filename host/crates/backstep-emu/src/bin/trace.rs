//! `backstep-emu-trace <firmware.elf> [max-instructions]`
//!
//! Runs a test firmware image on the emulator with the QEMU `mps2-an386`
//! memory map and prints the register state before every instruction in
//! exactly the format of `qemu-system-arm -d cpu`, so the two traces can be
//! compared line by line (`scripts/emu-difftest.py`). Stops at a
//! semihosting exit (`BKPT 0xAB` with r0 = 0x18).

use std::process::ExitCode;

use backstep_emu::elf::Elf;
use backstep_emu::memory::NoBus;
use backstep_emu::{Cpu, Memory, Stop};

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
    if cpu.reset(0).is_err() {
        eprintln!("backstep-emu-trace: no vector table at 0");
        return ExitCode::from(2);
    }
    let mut out = String::new();
    let mut bus = NoBus;
    for _ in 0..max {
        print_state(&cpu, &mut out);
        match cpu.step(&mut bus) {
            Ok(()) => {}
            Err(Stop::Breakpoint { imm: 0xAB, .. }) if cpu.r[0] == 0x18 => {
                print!("{out}");
                return ExitCode::SUCCESS;
            }
            Err(stop) => {
                print!("{out}");
                eprintln!("backstep-emu-trace: stopped: {stop:?}");
                return ExitCode::from(1);
            }
        }
    }
    print!("{out}");
    eprintln!("backstep-emu-trace: instruction limit reached");
    ExitCode::from(1)
}
