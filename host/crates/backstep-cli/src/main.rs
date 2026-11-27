//! `backstep` command-line tool.
//!
//! M0 ships `info` and `crc32`; `record`, `replay` and `gdb` arrive in
//! later milestones (see `docs/ROADMAP.md`).

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
