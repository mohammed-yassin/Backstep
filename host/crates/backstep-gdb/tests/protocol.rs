//! The remote protocol, packet by packet, against a real timeline (the
//! fixture recording of `backstep-replay`): framing, acknowledgements,
//! checksums, the target description, registers, memory, breakpoints,
//! watchpoints, forward and reverse movement, monitor commands.

use std::collections::VecDeque;
use std::fmt::Write as _;
use std::io;

use backstep_emu::elf::Elf;
use backstep_gdb::{serve, Connection, End, REGISTERS, TARGET_XML};
use backstep_replay::{parse, Timeline};

const FIXTURES: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../backstep-replay/tests/fixtures"
);

/// GDB's side, scripted: bytes in, bytes out.
struct Script {
    input: VecDeque<u8>,
    output: Vec<u8>,
}

impl Connection for Script {
    fn read_byte(&mut self) -> io::Result<Option<u8>> {
        Ok(self.input.pop_front())
    }
    fn write_all(&mut self, data: &[u8]) -> io::Result<()> {
        self.output.extend_from_slice(data);
        Ok(())
    }
    fn interrupt_requested(&mut self) -> bool {
        if self.input.front() == Some(&0x03) {
            self.input.pop_front();
            return true;
        }
        false
    }
}

fn frame(p: &str) -> String {
    format!("${p}#{:02x}", p.bytes().fold(0u8, u8::wrapping_add))
}

/// Run a session: each packet acknowledged with '+'. Returns the replies
/// (payloads, in order) and how the session ended.
fn session(packets: &[&str]) -> (Vec<String>, End) {
    let elf = Elf::parse(std::fs::read(format!("{FIXTURES}/irq_ambiguous.elf")).unwrap()).unwrap();
    let rec = parse(&std::fs::read(format!("{FIXTURES}/irq_ambiguous.bsr")).unwrap()).unwrap();
    let mut t = Timeline::new(&elf, &rec, 100_000_000).unwrap();
    let mut input = String::new();
    for p in packets {
        input.push_str(&frame(p));
        // The server's reply (and any O packets) are acknowledged.
        input.push_str("+++");
    }
    let mut conn = Script {
        input: input.into_bytes().into(),
        output: Vec::new(),
    };
    let end = serve(&mut t, &mut conn).unwrap();
    let out = String::from_utf8(conn.output).unwrap();
    let replies = out
        .split('$')
        .skip(1)
        .map(|s| s.split('#').next().unwrap().to_string())
        .collect();
    (replies, end)
}

fn sym(name: &str) -> u32 {
    let elf = Elf::parse(std::fs::read(format!("{FIXTURES}/irq_ambiguous.elf")).unwrap()).unwrap();
    elf.symbol(name).unwrap()
}

#[test]
fn handshake_registers_and_target_description() {
    let (r, end) = session(&[
        "qSupported:multiprocess+;swbreak+;hwbreak+",
        "qXfer:features:read:target.xml:0,40",
        "qXfer:features:read:target.xml:40,4000",
        "qXfer:features:read:other.xml:0,40",
        "?",
        "g",
        "pf",
        "p10",
        "p99",
        "qAttached",
        "qC",
        "vMustReplyEmpty",
        "D",
    ]);
    assert!(r[0].contains("ReverseStep+") && r[0].contains("ReverseContinue+"));
    assert!(r[0].contains("qXfer:features:read+"));
    assert!(r[1].starts_with('m') && TARGET_XML.starts_with(&r[1][1..]));
    assert!(r[2].starts_with('l'));
    assert_eq!(r[1].len() - 1 + r[2].len() - 1, TARGET_XML.len());
    assert_eq!(r[3], "E00");
    assert_eq!(r[4], "T05thread:1;");
    assert_eq!(r[5].len(), REGISTERS * 8);
    let pc = u32::from_le_bytes(u32::from_str_radix(&r[6], 16).unwrap().to_be_bytes());
    assert_eq!(&r[5][15 * 8..16 * 8], r[6], "p f is the pc of g");
    assert_eq!(pc & 1, 0);
    assert_eq!(r[7].len(), 8, "xpsr");
    assert_eq!(r[8], "E00");
    assert_eq!(
        (r[9].as_str(), r[10].as_str(), r[11].as_str()),
        ("1", "QC1", "")
    );
    assert_eq!(r[12], "OK");
    assert_eq!(end, End::Detached);
}

#[test]
fn memory_is_readable_and_history_is_read_only() {
    let (r, _) = session(&[
        "m20000000,10",
        "m40000000,4", // a peripheral: not in the emulated memory
        "m203ffffc,8", // the last 4 bytes of RAM, then nothing
        "M20000000,4:00000000",
        "G00",
        "P0=00000000",
        "k",
    ]);
    assert_eq!(r[0].len(), 32);
    assert_eq!(r[1], "E14");
    assert_eq!(r[2].len(), 8, "a partial read returns what exists");
    assert_eq!(&r[3..6], ["E01", "E01", "E01"]);
}

#[test]
fn breakpoints_run_forwards_and_backwards() {
    let stub = sym("timer0_stub") & !1;
    let set = format!("Z0,{stub:x},2");
    let (r, _) = session(&[
        &set,
        "c",
        "pf",
        "c",
        "bc",
        "bc",
        "bc",
        "bs",
        &format!("z0,{stub:x},2"),
        "c",
        "bs",
        "s",
        "s",
        "D",
    ]);
    let pc = format!("{:08x}", stub.swap_bytes());
    assert_eq!(r[0], "OK");
    assert_eq!(r[1], "T05swbreak:;thread:1;");
    assert_eq!(r[2], pc);
    assert_eq!(r[3], "T05swbreak:;thread:1;");
    assert_eq!(r[4], "T05swbreak:;thread:1;", "back to the first hit");
    assert_eq!(r[5], "T05replaylog:begin;thread:1;", "nothing before it");
    assert_eq!(r[6], "T05replaylog:begin;thread:1;");
    assert_eq!(
        r[7], "T05replaylog:begin;thread:1;",
        "no step before the start"
    );
    assert_eq!(r[8], "OK");
    assert_eq!(r[9], "T05replaylog:end;thread:1;");
    assert_eq!(r[10], "T05thread:1;");
    assert_eq!(r[11], "T05thread:1;");
    assert_eq!(r[12], "T05replaylog:end;thread:1;", "no step after the end");
}

#[test]
fn watchpoints_report_their_kind_and_address() {
    let count = sym("g_spin_count");
    let stop = sym("g_spin_stop");
    let (r, _) = session(&[
        &format!("Z2,{count:x},4"),
        "c",
        &format!("z2,{count:x},4"),
        &format!("Z3,{stop:x},4"),
        "c",
        &format!("z3,{stop:x},4"),
        &format!("Z4,{count:x},4"),
        "c",
        "bc",
        "D",
    ]);
    assert_eq!(r[1], format!("T05watch:{count:x};thread:1;"));
    assert_eq!(r[4], format!("T05rwatch:{stop:x};thread:1;"));
    assert_eq!(r[7], format!("T05awatch:{count:x};thread:1;"));
    assert!(r[8].starts_with("T05awatch:") || r[8].starts_with("T05replaylog:begin"));
}

#[test]
fn monitor_commands_show_and_set_the_position() {
    let hex = |s: &str| {
        s.bytes().fold(String::new(), |mut out, b| {
            let _ = write!(out, "{b:02x}");
            out
        })
    };
    let (r, _) = session(&[
        &format!("qRcmd,{}", hex("goto 1000")),
        &format!("qRcmd,{}", hex("position")),
        &format!("qRcmd,{}", hex("goto end")),
        "c",
        &format!("qRcmd,{}", hex("bogus")),
        "D",
    ]);
    let text = |s: &str| {
        String::from_utf8(
            (1..s.len())
                .step_by(2)
                .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
                .collect(),
        )
        .unwrap()
    };
    assert_eq!(text(&r[0]), "position 1000\n");
    assert_eq!(r[1], "OK");
    assert!(text(&r[2]).starts_with("position 1000 of 236432"));
    assert_eq!(text(&r[4]), "position 236432\n");
    assert_eq!(r[6], "T05replaylog:end;thread:1;");
    assert!(text(&r[7]).contains("monitor commands"));
}

#[test]
fn bad_checksums_are_refused_and_no_ack_mode_works() {
    let elf = Elf::parse(std::fs::read(format!("{FIXTURES}/irq_ambiguous.elf")).unwrap()).unwrap();
    let rec = parse(&std::fs::read(format!("{FIXTURES}/irq_ambiguous.bsr")).unwrap()).unwrap();
    let mut t = Timeline::new(&elf, &rec, 100_000_000).unwrap();
    let mut input = String::from("$qC#00"); // wrong checksum
    input.push_str(&frame("QStartNoAckMode"));
    input.push('+');
    input.push_str(&frame("qC"));
    input.push_str(&frame("D"));
    let mut conn = Script {
        input: input.into_bytes().into(),
        output: Vec::new(),
    };
    serve(&mut t, &mut conn).unwrap();
    let out = String::from_utf8(conn.output).unwrap();
    assert_eq!(
        out,
        format!("-+{}{}{}", frame("OK"), frame("QC1"), frame("OK"))
    );
}

#[test]
fn an_interrupt_stops_a_run() {
    let elf = Elf::parse(std::fs::read(format!("{FIXTURES}/irq_ambiguous.elf")).unwrap()).unwrap();
    let rec = parse(&std::fs::read(format!("{FIXTURES}/irq_ambiguous.bsr")).unwrap()).unwrap();
    let mut t = Timeline::new(&elf, &rec, 100_000_000).unwrap();
    let mut input = frame("c").into_bytes();
    input.push(0x03);
    input.extend(b"+");
    input.extend(frame("D").bytes());
    input.extend(b"+");
    let mut conn = Script {
        input: input.into(),
        output: Vec::new(),
    };
    serve(&mut t, &mut conn).unwrap();
    let out = String::from_utf8(conn.output).unwrap();
    assert!(out.contains(&frame("T02thread:1;")), "{out}");
}
