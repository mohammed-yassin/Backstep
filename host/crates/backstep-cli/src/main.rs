//! `backstep` command-line tool.
//!
//! `info`, `crc32`, `inspect` (a recording's contents), `replay` (re-run
//! a recording on the emulator and check it), `gdb` (debug a recording
//! with GDB, forwards and backwards), `why` (who wrote a variable) and
//! `timeline` (interrupts and faults).

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
    /// Debug a recording with GDB: check it as `replay` does, then serve
    /// the GDB remote protocol. GDB can step, continue, use breakpoints
    /// and watchpoints, and run backwards (reverse-step, reverse-continue,
    /// reverse-next, reverse-finish).
    ///
    /// In GDB: `target remote localhost:<port>`, or with `--stdio`:
    /// `target remote | backstep gdb --stdio <elf> <recording>`.
    Gdb {
        /// The firmware ELF that was recorded.
        elf: PathBuf,
        /// Recording file (.bsr).
        recording: PathBuf,
        /// TCP port to listen on (localhost only).
        #[arg(long, default_value_t = 3333)]
        port: u16,
        /// Talk to GDB on stdin and stdout instead of TCP.
        #[arg(long)]
        stdio: bool,
        /// Give up checking the recording after this many instructions.
        #[arg(long, default_value_t = 100_000_000)]
        max_instructions: u64,
    },
    /// Who wrote a variable: the last write before the end of the
    /// recording (or a position), with its source line, function and
    /// context (main code or which interrupt), the value before and after,
    /// every earlier write, and lost updates (a write based on a value
    /// another context changed in the meantime: a race).
    ///
    /// The variable is a global's name with any `.member` and `[index]`
    /// (`g_rx.limit`, `buf[3]`), or `0xADDRESS:SIZE`.
    Why {
        /// The firmware ELF that was recorded (with debug information).
        elf: PathBuf,
        /// Recording file (.bsr).
        recording: PathBuf,
        /// The variable.
        variable: String,
        /// Ask about this position instead of the end.
        #[arg(long)]
        at: Option<u64>,
        /// Print JSON.
        #[arg(long)]
        json: bool,
        /// Give up checking the recording after this many instructions.
        #[arg(long, default_value_t = 100_000_000)]
        max_instructions: u64,
    },
    /// The interrupt and fault timeline: every exception entry (recorded
    /// interrupts, faults with their causes, and the exceptions software
    /// raises) and return, with positions and source locations.
    Timeline {
        /// The firmware ELF that was recorded.
        elf: PathBuf,
        /// Recording file (.bsr).
        recording: PathBuf,
        /// Print JSON.
        #[arg(long)]
        json: bool,
        /// Give up checking the recording after this many instructions.
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
    println!("  interrupts      {}", rec.irqs.len());
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
                "replay: {} instructions on {}, {} peripheral loads and {} interrupts replayed, {} stores",
                report.instructions,
                report.target.name,
                report.loads,
                report.interrupts,
                report.stores
            );
            if report.retries > 0 {
                println!(
                    "replay: {} interrupt placements retried: their key also matched at other \
                     positions, and the first led to a divergence",
                    report.retries
                );
            }
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

fn load_elf(path: &PathBuf) -> Result<backstep_emu::elf::Elf, ExitCode> {
    let bytes = read(path)?;
    backstep_emu::elf::Elf::parse(bytes).map_err(|err| {
        eprintln!("backstep: {}: {err}", path.display());
        ExitCode::from(2)
    })
}

fn gdb(
    elf_path: &PathBuf,
    recording: &PathBuf,
    port: u16,
    stdio: bool,
    max_instructions: u64,
) -> Result<ExitCode, ExitCode> {
    let rec = load_recording(recording)?;
    let elf = load_elf(elf_path)?;
    let mut timeline =
        backstep_replay::Timeline::new(&elf, &rec, max_instructions).map_err(|err| {
            eprintln!("backstep gdb: the recording does not replay: {err}");
            ExitCode::from(1)
        })?;
    eprintln!(
        "backstep gdb: recording checked: {} instructions, {} interrupts",
        timeline.end(),
        rec.irqs.len()
    );
    let result = if stdio {
        backstep_gdb::serve(&mut timeline, &mut backstep_gdb::StdioConnection::new())
    } else {
        let listener = std::net::TcpListener::bind(("127.0.0.1", port)).map_err(|err| {
            eprintln!("backstep gdb: cannot listen on 127.0.0.1:{port}: {err}");
            ExitCode::from(2)
        })?;
        eprintln!(
            "backstep gdb: waiting for GDB on 127.0.0.1:{port} (in GDB: target remote localhost:{port})"
        );
        let (stream, _) = listener.accept().map_err(|err| {
            eprintln!("backstep gdb: {err}");
            ExitCode::from(2)
        })?;
        backstep_gdb::serve(&mut timeline, &mut backstep_gdb::TcpConnection::new(stream))
    };
    match result {
        Ok(_) => Ok(ExitCode::SUCCESS),
        Err(err) => {
            eprintln!("backstep gdb: connection: {err}");
            Err(ExitCode::from(2))
        }
    }
}

fn checked_timeline<'r>(
    elf: &backstep_emu::elf::Elf,
    rec: &'r backstep_replay::Recording,
    max_instructions: u64,
) -> Result<backstep_replay::Timeline<'r>, ExitCode> {
    backstep_replay::Timeline::new(elf, rec, max_instructions).map_err(|err| {
        eprintln!("backstep: the recording does not replay: {err}");
        ExitCode::from(1)
    })
}

fn hex_value(v: Option<u64>, size: u32) -> String {
    v.map_or_else(
        || format!("({size} bytes)"),
        |v| format!("{v:#0w$x}", w = 2 + 2 * size.min(8) as usize),
    )
}

fn print_access(a: &backstep_query::Access, size: u32) {
    println!(
        "  position {:>9}  {} by {}",
        a.position,
        if a.write { "write" } else { "read " },
        a.context
    );
    println!("                      {}", a.at);
    if a.write {
        println!(
            "                      {} -> {}",
            hex_value(a.before, size),
            hex_value(a.after, size)
        );
        if a.near_stack_pointer {
            println!(
                "                      through the stack (SP {:#010x}): the stack has grown into it (stack overflow)",
                a.sp
            );
        }
    }
}

fn why(
    elf_path: &PathBuf,
    recording: &PathBuf,
    variable: &str,
    at: Option<u64>,
    json: bool,
    max_instructions: u64,
) -> Result<ExitCode, ExitCode> {
    let rec = load_recording(recording)?;
    let elf = load_elf(elf_path)?;
    let debug = backstep_query::DebugInfo::new(&elf);
    let var = debug.variable(variable).map_err(|err| {
        eprintln!("backstep why: {err}");
        ExitCode::from(2)
    })?;
    let mut timeline = checked_timeline(&elf, &rec, max_instructions)?;
    let answer = backstep_query::why(&mut timeline, &debug, &var, at).map_err(|err| {
        eprintln!("backstep why: {err}");
        ExitCode::from(1)
    })?;
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&answer).unwrap_or_default()
        );
        return Ok(ExitCode::SUCCESS);
    }
    let size = answer.variable.size;
    println!(
        "{} ({}, {} bytes at {:#010x}) at position {}: {}",
        answer.variable.expression,
        answer.variable.type_name,
        size,
        answer.variable.address,
        answer.position,
        hex_value(answer.value, size)
    );
    match &answer.last_write {
        Some(w) => {
            println!("last written:");
            print_access(w, size);
        }
        None => println!("not written in the recording (the value is from before it started)"),
    }
    let plural = |n: usize, what: &str| format!("{n} {what}{}", if n == 1 { "" } else { "s" });
    println!(
        "{} and {} in the recording up to there",
        plural(answer.writes.len(), "write"),
        plural(answer.reads, "read")
    );
    for l in &answer.lost_updates {
        println!("lost update: a write based on a stale read");
        println!("  read:");
        print_access(&l.read, size);
        println!("  overwritten writes of another context:");
        for w in &l.lost {
            print_access(w, size);
        }
        println!("  the write that overwrote them:");
        print_access(&l.write, size);
    }
    Ok(ExitCode::SUCCESS)
}

fn timeline(
    elf_path: &PathBuf,
    recording: &PathBuf,
    json: bool,
    max_instructions: u64,
) -> Result<ExitCode, ExitCode> {
    let rec = load_recording(recording)?;
    let elf = load_elf(elf_path)?;
    let debug = backstep_query::DebugInfo::new(&elf);
    let mut t = checked_timeline(&elf, &rec, max_instructions)?;
    let events = backstep_query::events(&mut t, &debug).map_err(|err| {
        eprintln!("backstep timeline: {err}");
        ExitCode::from(1)
    })?;
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&events).unwrap_or_default()
        );
        return Ok(ExitCode::SUCCESS);
    }
    let mut depth = 0usize;
    for e in &events {
        let indent = "  ".repeat(depth);
        let text = match &e.kind {
            backstep_query::EventKind::Start => format!("start               {}", e.at),
            backstep_query::EventKind::End => format!("end                 {}", e.at),
            backstep_query::EventKind::Entry {
                name,
                fault,
                causes,
                tail_chained,
                nested,
                ..
            } => {
                let mut how = Vec::new();
                if *fault {
                    how.push("FAULT".to_string());
                }
                if !causes.is_empty() {
                    how.push(causes.join(","));
                }
                if *tail_chained {
                    how.push("tail-chained".into());
                }
                if *nested {
                    how.push("nested".into());
                }
                let how = if how.is_empty() {
                    String::new()
                } else {
                    format!(" ({})", how.join(", "))
                };
                depth += 1;
                match &e.previous {
                    Some(p) => format!("{indent}-> {name}{how}  at {}, after {p}", e.at),
                    None => format!("{indent}-> {name}{how}  from {}", e.at),
                }
            }
            backstep_query::EventKind::Return { name, .. } => {
                depth = depth.saturating_sub(1);
                format!("{}<- {name}", "  ".repeat(depth))
            }
        };
        println!("{:>10}  {text}", e.position);
    }
    Ok(ExitCode::SUCCESS)
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
        Command::Gdb {
            elf,
            recording,
            port,
            stdio,
            max_instructions,
        } => gdb(&elf, &recording, port, stdio, max_instructions).unwrap_or_else(|code| code),
        Command::Why {
            elf,
            recording,
            variable,
            at,
            json,
            max_instructions,
        } => {
            why(&elf, &recording, &variable, at, json, max_instructions).unwrap_or_else(|code| code)
        }
        Command::Timeline {
            elf,
            recording,
            json,
            max_instructions,
        } => timeline(&elf, &recording, json, max_instructions).unwrap_or_else(|code| code),
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
