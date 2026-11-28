//! `backstep` command-line tool.
//!
//! `info`, `crc32`, `inspect` (a recording's contents) and `replay`
//! (re-run a recording on the emulator and check it). `gdb` arrives in M5
//! (see `docs/ROADMAP.md`).

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};

/// Record and replay microcontroller firmware, including stepping backwards.
#[derive(Debug, Parser)]
#[command(name = "backstep", version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Show version and supported targets.
    Info,
    /// Print the CRC-32 of a file, as computed by the on-device recorder.
    Crc32 {
        /// File to checksum.
        file: PathBuf,
    },
    /// Check a recording's integrity and summarise it.
    Inspect {
        /// Recording file (.bsr).
        recording: PathBuf,
    },
    /// Re-run a recording on the emulator and check that it reproduces the
    /// recorded execution: every peripheral load and the end state.
    ///
    /// Exit status: 0 reproduced, 1 diverged, 2 unusable input.
    Replay {
        /// The firmware ELF that was recorded.
        elf: PathBuf,
        /// Recording file (.bsr).
        recording: PathBuf,
        /// Write the register state before every instruction here
        /// (QEMU `-d cpu` format).
        #[arg(long)]
        trace: Option<PathBuf>,
        /// Give up after this many instructions.
        #[arg(long, default_value_t = 100_000_000)]
        max_instructions: u64,
    },
}

fn read(path: &PathBuf) -> Result<Vec<u8>, ExitCode> {
    std::fs::read(path).map_err(|err| {
        eprintln!("backstep: cannot read {}: {err}", path.display());
        ExitCode::from(2)
    })
}

fn load_recording(path: &PathBuf) -> Result<backstep_replay::Recording, ExitCode> {
    let bytes = read(path)?;
    backstep_replay::parse(&bytes).map_err(|err| {
        eprintln!("backstep: {}: {err}", path.display());
        ExitCode::from(2)
    })
}

fn inspect(path: &PathBuf) -> Result<ExitCode, ExitCode> {
    let rec = load_recording(path)?;
    let target = backstep_replay::target::by_id(rec.target_id).map_or_else(
        || format!("unknown ({})", rec.target_id),
        |t| t.name.to_string(),
    );
    println!("recording {}: intact (all checksums match)", path.display());
    println!("  target          {target}");
    println!(
        "  firmware        {} bytes at {:#010x}, CRC-32 {:08X}",
        rec.image_len, rec.image_start, rec.image_crc32
    );
    println!(
        "  start           pc {:#010x}, sp {:#010x}",
        rec.snapshot.cpu.pc, rec.snapshot.cpu.sp
    );
    for r in &rec.snapshot.regions {
        println!(
            "  RAM             {} bytes at {:#010x}",
            r.bytes.len(),
            r.address
        );
    }
    println!("  peripheral loads {}", rec.events.len());
    println!(
        "  end             lr {:#010x}, sp {:#010x}, RAM CRC-32 {:08X}",
        rec.end.cpu.lr, rec.end.cpu.sp, rec.end.ram_crc32
    );
    Ok(ExitCode::SUCCESS)
}

fn replay(
    elf: &PathBuf,
    recording: &PathBuf,
    trace: Option<&PathBuf>,
    max_instructions: u64,
) -> Result<ExitCode, ExitCode> {
    let rec = load_recording(recording)?;
    let elf_bytes = read(elf)?;
    let elf = backstep_emu::elf::Elf::parse(elf_bytes).map_err(|err| {
        eprintln!("backstep: {}: {err}", elf.display());
        ExitCode::from(2)
    })?;
    let mut trace_file = match trace {
        Some(path) => Some(std::io::BufWriter::new(
            std::fs::File::create(path).map_err(|err| {
                eprintln!("backstep: cannot create {}: {err}", path.display());
                ExitCode::from(2)
            })?,
        )),
        None => None,
    };
    let opts = backstep_replay::Options {
        max_instructions,
        trace: trace_file.as_mut().map(|f| f as &mut dyn std::io::Write),
    };
    match backstep_replay::replay(&elf, &rec, opts) {
        Ok(report) => {
            println!(
                "replay: {} instructions on {}, {} peripheral loads replayed, {} stores",
                report.instructions, report.target.name, report.loads, report.stores
            );
            println!(
                "replay: end state matches the recording (registers, RAM CRC-32 {:08X})",
                report.ram_crc32
            );
            println!("replay: PASS");
            Ok(ExitCode::SUCCESS)
        }
        Err(
            err @ (backstep_replay::ReplayError::UnknownTarget(_)
            | backstep_replay::ReplayError::Elf(_)
            | backstep_replay::ReplayError::FirmwareMismatch { .. }),
        ) => {
            eprintln!("replay: {err}");
            Err(ExitCode::from(2))
        }
        Err(err) => {
            eprintln!("replay: {err}");
            println!("replay: FAIL");
            Ok(ExitCode::from(1))
        }
    }
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.command {
        Command::Info => {
            println!("backstep {}", backstep_core::VERSION);
            println!("supported targets:");
            for target in backstep_core::SUPPORTED_TARGETS {
                println!("  {:<16} {}", target.name, target.description);
            }
            ExitCode::SUCCESS
        }
        Command::Crc32 { file } => match std::fs::read(&file) {
            Ok(bytes) => {
                println!(
                    "{:08X}  {}",
                    backstep_core::crc32::crc32(&bytes),
                    file.display()
                );
                ExitCode::SUCCESS
            }
            Err(err) => {
                eprintln!("backstep: cannot read {}: {err}", file.display());
                ExitCode::from(2)
            }
        },
        Command::Inspect { recording } => inspect(&recording).unwrap_or_else(|code| code),
        Command::Replay {
            elf,
            recording,
            trace,
            max_instructions,
        } => replay(&elf, &recording, trace.as_ref(), max_instructions).unwrap_or_else(|code| code),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn cli_definition_is_valid() {
        Cli::command().debug_assert();
    }
}
