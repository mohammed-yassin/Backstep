//! A GDB remote serial protocol (RSP) server over a [`Timeline`]: GDB
//! debugs a recording as if it were a running Cortex-M, and can also run
//! it backwards (`reverse-stepi`, `reverse-continue`, and the
//! `reverse-next` and `reverse-finish` GDB builds from them).
//!
//! The recording is history: memory and registers can be read but not
//! written. Supported: registers (`g`, `p`), memory (`m`), software and
//! hardware breakpoints (`Z0`, `Z1`), write, read and access watchpoints
//! (`Z2`-`Z4`), `c`, `s`, `vCont`, `bc`, `bs`, the target description
//! (`qXfer:features:read`), no-ack mode, interrupting a run (Ctrl-C), and
//! `monitor` commands (`qRcmd`) to show or set the position.
//! Protocol: <https://sourceware.org/gdb/current/onlinedocs/gdb.html/Remote-Protocol.html>.

#![allow(clippy::doc_markdown)]

use std::collections::HashMap;
use std::fmt::Write as _;
use std::io;

use backstep_emu::memory::WatchKind;
use backstep_replay::{Stop, Timeline, Watchpoint};

/// A byte stream to GDB.
pub trait Connection {
    /// The next byte, waiting for it; `None` when GDB has gone.
    ///
    /// # Errors
    /// I/O errors.
    fn read_byte(&mut self) -> io::Result<Option<u8>>;
    /// Send bytes.
    ///
    /// # Errors
    /// I/O errors.
    fn write_all(&mut self, data: &[u8]) -> io::Result<()>;
    /// Whether GDB has asked to stop a run (a 0x03 byte waiting), without
    /// waiting. Streams that cannot tell return false.
    fn interrupt_requested(&mut self) -> bool;
}

/// The target description: Cortex-M registers in `g` packet order.
pub const TARGET_XML: &str = r#"<?xml version="1.0"?>
<!DOCTYPE target SYSTEM "gdb-target.dtd">
<target version="1.0">
  <architecture>arm</architecture>
  <feature name="org.gnu.gdb.arm.m-profile">
    <reg name="r0" bitsize="32" regnum="0"/>
    <reg name="r1" bitsize="32"/>
    <reg name="r2" bitsize="32"/>
    <reg name="r3" bitsize="32"/>
    <reg name="r4" bitsize="32"/>
    <reg name="r5" bitsize="32"/>
    <reg name="r6" bitsize="32"/>
    <reg name="r7" bitsize="32"/>
    <reg name="r8" bitsize="32"/>
    <reg name="r9" bitsize="32"/>
    <reg name="r10" bitsize="32"/>
    <reg name="r11" bitsize="32"/>
    <reg name="r12" bitsize="32"/>
    <reg name="sp" bitsize="32" type="data_ptr"/>
    <reg name="lr" bitsize="32"/>
    <reg name="pc" bitsize="32" type="code_ptr"/>
    <reg name="xpsr" bitsize="32"/>
  </feature>
  <feature name="org.gnu.gdb.arm.m-system">
    <reg name="msp" bitsize="32" type="data_ptr"/>
    <reg name="psp" bitsize="32" type="data_ptr"/>
  </feature>
  <feature name="org.backstep.m-special">
    <reg name="primask" bitsize="32" group="system"/>
    <reg name="basepri" bitsize="32" group="system"/>
    <reg name="faultmask" bitsize="32" group="system"/>
    <reg name="control" bitsize="32" group="system"/>
  </feature>
</target>
"#;

/// Registers in the target description.
pub const REGISTERS: usize = 23;

/// Register `n` of the description at the timeline's position.
fn register(t: &Timeline<'_>, n: usize) -> Option<u32> {
    let cpu = t.cpu();
    Some(match n {
        0..=15 => cpu.r[n],
        16 => cpu.xpsr(),
        17 => cpu.msp(),
        18 => cpu.psp(),
        19 => u32::from(cpu.primask),
        20 => u32::from(cpu.basepri),
        21 => u32::from(cpu.faultmask),
        22 => cpu.control,
        _ => return None,
    })
}

fn hex_le(v: u32) -> String {
    v.to_le_bytes().iter().fold(String::new(), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}

fn hex(data: &[u8]) -> String {
    data.iter().fold(String::new(), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}

fn unhex(s: &str) -> Option<Vec<u8>> {
    if s.len() % 2 != 0 {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok())
        .collect()
}

/// `addr,len` (hex).
fn addr_len(s: &str) -> Option<(u32, u32)> {
    let (a, l) = s.split_once(',')?;
    Some((
        u32::from_str_radix(a, 16).ok()?,
        u32::from_str_radix(l, 16).ok()?,
    ))
}

/// Escape `#`, `$`, `}` and `*` in a binary reply (`qXfer`).
fn escape(data: &str) -> String {
    let mut out = String::with_capacity(data.len());
    for c in data.chars() {
        if matches!(c, '#' | '$' | '}' | '*') {
            out.push('}');
            out.push(char::from(c as u8 ^ 0x20));
        } else {
            out.push(c);
        }
    }
    out
}

/// What a breakpoint was set as (the stop reply names it).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BreakKind {
    Software,
    Hardware,
}

/// Serves one GDB session.
pub struct Server<'t, 'r> {
    timeline: &'t mut Timeline<'r>,
    ack: bool,
    breakpoints: HashMap<u32, BreakKind>,
    /// Watchpoints as GDB set them.
    watchpoints: Vec<Watchpoint>,
    last_stop: Stop,
}

/// Why a session ended.
#[derive(Debug, PartialEq, Eq)]
pub enum End {
    /// GDB detached or killed the target.
    Detached,
    /// The connection closed.
    Closed,
}

impl<'t, 'r> Server<'t, 'r> {
    /// A server for `timeline`.
    pub fn new(timeline: &'t mut Timeline<'r>) -> Self {
        Server {
            timeline,
            ack: true,
            breakpoints: HashMap::new(),
            watchpoints: Vec::new(),
            last_stop: Stop::Step,
        }
    }

    /// Talk to GDB until it detaches or goes away.
    ///
    /// # Errors
    /// I/O errors of the connection.
    pub fn serve(&mut self, conn: &mut dyn Connection) -> io::Result<End> {
        loop {
            let packet = match read_packet(conn, self.ack)? {
                Incoming::Packet(p) => p,
                Incoming::Nothing => continue,
                Incoming::Closed => return Ok(End::Closed),
            };
            match self.handle(&packet, conn) {
                Reply::Send(r) => send(conn, &r, self.ack)?,
                Reply::SendThenDetach(r) => {
                    send(conn, &r, self.ack)?;
                    return Ok(End::Detached);
                }
                Reply::Nothing => {}
                Reply::Quit => return Ok(End::Detached),
            }
        }
    }

    fn handle(&mut self, packet: &str, conn: &mut dyn Connection) -> Reply {
        let reply = |s: &str| Reply::Send(s.to_string());
        let (head, rest) = packet.split_at(packet.find([':', ';', ',']).unwrap_or(packet.len()));
        match packet.as_bytes().first().copied() {
            Some(b'?') => Reply::Send(self.stop_reply(self.last_stop)),
            Some(b'g') => Reply::Send(
                (0..REGISTERS)
                    .map(|n| hex_le(register(self.timeline, n).unwrap_or(0)))
                    .collect(),
            ),
            Some(b'p') => match usize::from_str_radix(&packet[1..], 16)
                .ok()
                .and_then(|n| register(self.timeline, n))
            {
                Some(v) => Reply::Send(hex_le(v)),
                None => reply("E00"),
            },
            Some(b'm') => match addr_len(&packet[1..]) {
                Some((a, l)) => {
                    let bytes: Vec<u8> = self
                        .timeline
                        .read_memory(a, l.min(0x800))
                        .into_iter()
                        .map_while(|b| b)
                        .collect();
                    if bytes.is_empty() && l > 0 {
                        reply("E14")
                    } else {
                        Reply::Send(hex(&bytes))
                    }
                }
                None => reply("E01"),
            },
            // History cannot be changed.
            Some(b'M' | b'X' | b'G' | b'P') => reply("E01"),
            Some(b'Z' | b'z') => self.point(packet),
            Some(b'c') => self.go(conn, Direction::Forward, true),
            Some(b's') => self.go(conn, Direction::Forward, false),
            Some(b'b') if packet == "bc" => self.go(conn, Direction::Backward, true),
            Some(b'b') if packet == "bs" => self.go(conn, Direction::Backward, false),
            Some(b'D') => Reply::SendThenDetach("OK".into()),
            Some(b'k') => Reply::Quit,
            Some(b'H' | b'T') => reply("OK"),
            _ => match head {
                "qSupported" => reply(
                    "PacketSize=4000;qXfer:features:read+;swbreak+;hwbreak+;\
                     ReverseStep+;ReverseContinue+;QStartNoAckMode+;vContSupported+",
                ),
                "QStartNoAckMode" => {
                    // Acknowledged with acks still on, then off.
                    if send(conn, "OK", true).is_err() {
                        return Reply::Quit;
                    }
                    self.ack = false;
                    Reply::Nothing
                }
                "qXfer" => Self::xfer(rest),
                "qAttached" => reply("1"),
                "qC" => reply("QC1"),
                "qfThreadInfo" => reply("m1"),
                "qsThreadInfo" => reply("l"),
                "qSymbol" => reply("OK"),
                "qRcmd" => self.monitor(&rest[1..], conn),
                "vCont?" => reply("vCont;c;C;s;S"),
                "vCont" => match rest.as_bytes().get(1) {
                    Some(b'c' | b'C') => self.go(conn, Direction::Forward, true),
                    Some(b's' | b'S') => self.go(conn, Direction::Forward, false),
                    _ => reply(""),
                },
                "vKill" => Reply::SendThenDetach("OK".into()),
                _ => reply(""), // not supported
            },
        }
    }

    fn xfer(rest: &str) -> Reply {
        // :features:read:target.xml:offset,length
        let mut parts = rest.trim_start_matches(':').splitn(4, ':');
        let (Some("features"), Some("read"), Some(annex), Some(range)) =
            (parts.next(), parts.next(), parts.next(), parts.next())
        else {
            return Reply::Send(String::new());
        };
        if annex != "target.xml" {
            return Reply::Send("E00".into());
        }
        let Some((offset, len)) = addr_len(range) else {
            return Reply::Send("E01".into());
        };
        let (offset, len) = (offset as usize, len as usize);
        let doc = TARGET_XML.as_bytes();
        if offset >= doc.len() {
            return Reply::Send("l".into());
        }
        let end = (offset + len).min(doc.len());
        let chunk = String::from_utf8_lossy(&doc[offset..end]).into_owned();
        let marker = if end == doc.len() { 'l' } else { 'm' };
        Reply::Send(format!("{marker}{}", escape(&chunk)))
    }

    fn point(&mut self, packet: &str) -> Reply {
        let insert = packet.starts_with('Z');
        let mut fields = packet[1..].split([',', ';']);
        let (Some(kind), Some(addr), Some(len)) = (fields.next(), fields.next(), fields.next())
        else {
            return Reply::Send("E01".into());
        };
        let (Ok(addr), Ok(len)) = (u32::from_str_radix(addr, 16), u32::from_str_radix(len, 16))
        else {
            return Reply::Send("E01".into());
        };
        match kind {
            "0" | "1" => {
                let as_kind = if kind == "0" {
                    BreakKind::Software
                } else {
                    BreakKind::Hardware
                };
                if insert {
                    self.breakpoints.insert(addr & !1, as_kind);
                    self.timeline.add_breakpoint(addr);
                } else {
                    self.breakpoints.remove(&(addr & !1));
                    self.timeline.remove_breakpoint(addr);
                }
            }
            "2" | "3" | "4" => {
                let w = Watchpoint {
                    address: addr,
                    len: len.max(1),
                    kind: match kind {
                        "2" => WatchKind::Write,
                        "3" => WatchKind::Read,
                        _ => WatchKind::Access,
                    },
                };
                if insert {
                    self.watchpoints.push(w);
                    self.timeline.add_watchpoint(w);
                } else {
                    self.watchpoints.retain(|x| *x != w);
                    self.timeline.remove_watchpoint(w);
                }
            }
            _ => return Reply::Send(String::new()),
        }
        Reply::Send("OK".into())
    }

    fn go(&mut self, conn: &mut dyn Connection, dir: Direction, run: bool) -> Reply {
        let mut interrupt = || conn.interrupt_requested();
        let result = match (dir, run) {
            (Direction::Forward, false) => self.timeline.step(),
            (Direction::Backward, false) => self.timeline.step_back(),
            (Direction::Forward, true) => self.timeline.run(&mut interrupt),
            (Direction::Backward, true) => self.timeline.run_back(&mut interrupt),
        };
        match result {
            Ok(stop) => {
                if let Stop::Watch(_) = stop {
                    // GDB treats Arm watchpoints as stopping before the
                    // accessing instruction, which it then steps over
                    // itself (forwards or backwards). The timeline stops
                    // after it going forwards and before it going
                    // backwards: report the other side.
                    let p = match dir {
                        Direction::Forward => self.timeline.position() - 1,
                        Direction::Backward => self.timeline.position() + 1,
                    };
                    if let Err(e) = self.timeline.seek(p) {
                        return self.failed(conn, &e);
                    }
                }
                self.last_stop = stop;
                Reply::Send(self.stop_reply(stop))
            }
            // A checked recording does not fail to replay; if it does,
            // say so and stop as at the end.
            Err(e) => self.failed(conn, &e),
        }
    }

    /// Report a replay error to GDB's console and stop as at the end.
    fn failed(&mut self, conn: &mut dyn Connection, e: &backstep_replay::ReplayError) -> Reply {
        let text = format!("O{}", hex(format!("backstep: {e}\n").as_bytes()));
        if send(conn, &text, self.ack).is_err() {
            return Reply::Quit;
        }
        self.last_stop = Stop::End;
        Reply::Send(self.stop_reply(Stop::End))
    }

    fn stop_reply(&self, stop: Stop) -> String {
        let thread = "thread:1;";
        match stop {
            Stop::Step => format!("T05{thread}"),
            Stop::Breakpoint => {
                let pc = self.timeline.cpu().r[15];
                match self.breakpoints.get(&pc) {
                    Some(BreakKind::Hardware) => format!("T05hwbreak:;{thread}"),
                    _ => format!("T05swbreak:;{thread}"),
                }
            }
            Stop::Watch(hit) => {
                // Report the watched range's address that was accessed.
                let w = self.watchpoints.iter().find(|w| {
                    u64::from(hit.address) < u64::from(w.address) + u64::from(w.len)
                        && u64::from(hit.address) + u64::from(hit.size) > u64::from(w.address)
                });
                let (name, address) = match w {
                    Some(w) => (
                        match w.kind {
                            WatchKind::Write => "watch",
                            WatchKind::Read => "rwatch",
                            WatchKind::Access => "awatch",
                        },
                        hit.address.max(w.address),
                    ),
                    None => ("watch", hit.address),
                };
                format!("T05{name}:{address:x};{thread}")
            }
            Stop::End => format!("T05replaylog:end;{thread}"),
            Stop::Start => format!("T05replaylog:begin;{thread}"),
            Stop::Interrupted => format!("T02{thread}"),
        }
    }

    fn monitor(&mut self, hex_command: &str, conn: &mut dyn Connection) -> Reply {
        let command = unhex(hex_command)
            .map(|b| String::from_utf8_lossy(&b).trim().to_string())
            .unwrap_or_default();
        let mut words = command.split_whitespace();
        let text = match (words.next(), words.next()) {
            (Some("position" | "pos") | None, None) => format!(
                "position {} of {} (instructions since the start of the recording); {} interrupts delivered\n",
                self.timeline.position(),
                self.timeline.end(),
                self.timeline.interrupts_delivered()
            ),
            (Some("goto"), Some(n)) => {
                let target = match n {
                    "start" => Some(0),
                    "end" => Some(self.timeline.end()),
                    _ => n.parse().ok(),
                };
                match target {
                    Some(p) => match self.timeline.seek(p) {
                        Ok(()) => {
                            self.last_stop = Stop::Step;
                            format!("position {}\n", self.timeline.position())
                        }
                        Err(e) => format!("backstep: {e}\n"),
                    },
                    None => "usage: monitor goto <position>|start|end\n".to_string(),
                }
            }
            _ => "backstep monitor commands: position, goto <n>|start|end\n".to_string(),
        };
        if send(conn, &format!("O{}", hex(text.as_bytes())), self.ack).is_err() {
            return Reply::Quit;
        }
        Reply::Send("OK".into())
    }
}

#[derive(Clone, Copy)]
enum Direction {
    Forward,
    Backward,
}

enum Reply {
    Send(String),
    SendThenDetach(String),
    /// Already answered.
    Nothing,
    Quit,
}

/// What arrived from GDB.
enum Incoming {
    Packet(String),
    /// Not a packet: an interrupt byte with nothing running, or a packet
    /// with a bad checksum (GDB resends it).
    Nothing,
    /// The stream ended.
    Closed,
}

/// Read one packet.
fn read_packet(conn: &mut dyn Connection, ack: bool) -> io::Result<Incoming> {
    loop {
        let Some(b) = conn.read_byte()? else {
            return Ok(Incoming::Closed);
        };
        match b {
            b'$' => break,
            0x03 => return Ok(Incoming::Nothing),
            _ => {} // acks, noise
        }
    }
    let mut payload = Vec::new();
    loop {
        let Some(b) = conn.read_byte()? else {
            return Ok(Incoming::Closed);
        };
        if b == b'#' {
            break;
        }
        payload.push(b);
    }
    let mut sum = [0u8; 2];
    for s in &mut sum {
        let Some(b) = conn.read_byte()? else {
            return Ok(Incoming::Closed);
        };
        *s = b;
    }
    let expected = u8::from_str_radix(std::str::from_utf8(&sum).unwrap_or(""), 16).ok();
    let actual = payload.iter().fold(0u8, |a, b| a.wrapping_add(*b));
    if ack {
        conn.write_all(if expected == Some(actual) { b"+" } else { b"-" })?;
        if expected != Some(actual) {
            return Ok(Incoming::Nothing);
        }
    }
    // Run-length encoding is only used by stubs, not by GDB.
    Ok(Incoming::Packet(
        String::from_utf8_lossy(&payload).into_owned(),
    ))
}

fn send(conn: &mut dyn Connection, payload: &str, ack: bool) -> io::Result<()> {
    let sum = payload.bytes().fold(0u8, u8::wrapping_add);
    let framed = format!("${payload}#{sum:02x}");
    loop {
        conn.write_all(framed.as_bytes())?;
        if !ack {
            return Ok(());
        }
        // Wait for the acknowledgement; resend on '-'.
        loop {
            match conn.read_byte()? {
                Some(b'+') | None => return Ok(()),
                Some(b'-') => break,
                Some(_) => {}
            }
        }
    }
}

/// Serve `timeline` on `conn` until GDB detaches.
///
/// # Errors
/// I/O errors of the connection.
pub fn serve(timeline: &mut Timeline<'_>, conn: &mut dyn Connection) -> io::Result<End> {
    Server::new(timeline).serve(conn)
}

mod conn;
pub use conn::{StdioConnection, TcpConnection};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn helpers() {
        assert_eq!(hex_le(0x1234_5678), "78563412");
        assert_eq!(unhex("6869").unwrap(), b"hi");
        assert_eq!(unhex("6"), None);
        assert_eq!(addr_len("20000000,10"), Some((0x2000_0000, 0x10)));
        assert_eq!(escape("a#b$c}d*"), "a}\u{3}b}\u{4}c}]d}\n");
        assert!(TARGET_XML.contains("org.gnu.gdb.arm.m-profile"));
        assert_eq!(TARGET_XML.matches("<reg ").count(), REGISTERS);
    }
}
