//! The recording file (`docs/adr/0008-recording-format.md`): parsing with
//! full integrity checks, and writing (used by tests and tools).
//!
//! All integers are little-endian `u32`. Layout:
//!
//! ```text
//! header   magic "BSRC", version 1, header bytes (16), chunk count
//! chunk*   type (4 ASCII bytes), payload length, payload, CRC-32 of payload
//! trailer  CRC-32 of every byte before it
//! ```
//!
//! Chunks, in this order: `TGTD` (target and firmware identity), `SNAP`
//! (the starting-state snapshot of ADR 0005), `MMIO` (the peripheral
//! loads), `END ` (the state replay must reach).

use std::fmt;

use backstep_core::crc32::{crc32, update};

/// "BSRC".
pub const MAGIC: u32 = 0x4352_5342;
/// The format version this code reads and writes.
pub const VERSION: u32 = 1;
const SNAPSHOT_MAGIC: u32 = 0x504E_5342; // "BSNP"
const SNAPSHOT_HEADER_BYTES: usize = 524;
const CPU_WORDS: usize = 23;

/// Chunk type codes.
pub mod chunk {
    /// Target and firmware identity.
    pub const TARGET: [u8; 4] = *b"TGTD";
    /// Starting-state snapshot.
    pub const SNAPSHOT: [u8; 4] = *b"SNAP";
    /// Peripheral loads.
    pub const MMIO: [u8; 4] = *b"MMIO";
    /// End state.
    pub const END: [u8; 4] = *b"END ";
}

/// Core registers as the recorder saves them (`struct bs_cpu_state`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CpuState {
    /// r0-r12 (the recorder stores 0 for the scratch registers r0-r3, r12).
    pub r: [u32; 13],
    /// The active stack pointer.
    pub sp: u32,
    /// Link register.
    pub lr: u32,
    /// Resume address.
    pub pc: u32,
    /// xPSR (flags, GE; IPSR).
    pub xpsr: u32,
    /// Main stack pointer.
    pub msp: u32,
    /// Process stack pointer.
    pub psp: u32,
    /// PRIMASK.
    pub primask: u32,
    /// BASEPRI.
    pub basepri: u32,
    /// FAULTMASK.
    pub faultmask: u32,
    /// CONTROL.
    pub control: u32,
}

impl CpuState {
    fn words(&self) -> [u32; CPU_WORDS] {
        let mut w = [0u32; CPU_WORDS];
        w[..13].copy_from_slice(&self.r);
        w[13..].copy_from_slice(&[
            self.sp,
            self.lr,
            self.pc,
            self.xpsr,
            self.msp,
            self.psp,
            self.primask,
            self.basepri,
            self.faultmask,
            self.control,
        ]);
        w
    }

    fn from_words(w: &[u32]) -> Self {
        let mut r = [0u32; 13];
        r.copy_from_slice(&w[..13]);
        CpuState {
            r,
            sp: w[13],
            lr: w[14],
            pc: w[15],
            xpsr: w[16],
            msp: w[17],
            psp: w[18],
            primask: w[19],
            basepri: w[20],
            faultmask: w[21],
            control: w[22],
        }
    }
}

/// System Control Space state in the snapshot (`struct bs_scs_state`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScsState {
    /// VTOR.
    pub vtor: u32,
    /// AIRCR.
    pub aircr: u32,
    /// CCR.
    pub ccr: u32,
    /// SHPR1-3.
    pub shpr: [u32; 3],
    /// SHCSR.
    pub shcsr: u32,
    /// NVIC ISER0-7.
    pub nvic_iser: [u32; 8],
    /// NVIC IPR, one byte per interrupt.
    pub nvic_ipr: Vec<u8>,
    /// SysTick CSR and RVR.
    pub syst: [u32; 2],
    /// MPU CTRL, RBAR0-7, RASR0-7.
    pub mpu: [u32; 17],
    /// CPACR.
    pub cpacr: u32,
}

/// A RAM region with its contents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Region {
    /// Start address.
    pub address: u32,
    /// Contents.
    pub bytes: Vec<u8>,
}

/// The starting state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    /// Registers at the start.
    pub cpu: CpuState,
    /// SCS state at the start.
    pub scs: ScsState,
    /// The program's RAM at the start.
    pub regions: Vec<Region>,
}

/// One recorded peripheral load (`struct bs_mmio_event`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MmioEvent {
    /// Address read.
    pub address: u32,
    /// Value (zero-extended).
    pub value: u32,
    /// Address of the load instruction.
    pub pc: u32,
    /// Size in bytes (1, 2, 4).
    pub size: u8,
}

/// The state at the end of the recording.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EndState {
    /// Registers (scratch registers stored as 0).
    pub cpu: CpuState,
    /// CRC-32 of the regions' contents, in order.
    pub ram_crc32: u32,
    /// The RAM regions checksummed: (address, length).
    pub regions: Vec<(u32, u32)>,
}

/// A parsed, integrity-checked recording.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recording {
    /// Target (see [`crate::target`]).
    pub target_id: u32,
    /// CRC-32 of the firmware image.
    pub image_crc32: u32,
    /// Start of the firmware image.
    pub image_start: u32,
    /// Length of the firmware image.
    pub image_len: u32,
    /// Starting state.
    pub snapshot: Snapshot,
    /// Peripheral loads, in program order.
    pub events: Vec<MmioEvent>,
    /// Loads the recorder could not store (a usable recording has 0).
    pub dropped: u32,
    /// The state replay must reach.
    pub end: EndState,
}

/// Why a file is not a valid recording.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FormatError {
    /// The file ends early.
    Truncated,
    /// Not a Backstep recording.
    BadMagic,
    /// A version this tool does not read.
    UnsupportedVersion(u32),
    /// The trailer CRC does not match the file.
    FileChecksum,
    /// A chunk's CRC does not match its payload.
    ChunkChecksum(String),
    /// The snapshot's region CRC does not match its contents.
    SnapshotChecksum,
    /// A chunk is missing, out of order or malformed.
    Malformed(String),
    /// The recorder lost loads (its log was full).
    Dropped(u32),
}

impl fmt::Display for FormatError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FormatError::Truncated => write!(f, "the file is truncated"),
            FormatError::BadMagic => write!(f, "not a Backstep recording (bad magic)"),
            FormatError::UnsupportedVersion(v) => write!(f, "unsupported format version {v}"),
            FormatError::FileChecksum => write!(f, "file checksum mismatch (corrupted)"),
            FormatError::ChunkChecksum(c) => write!(f, "chunk {c} checksum mismatch (corrupted)"),
            FormatError::SnapshotChecksum => {
                write!(f, "snapshot RAM checksum mismatch (corrupted)")
            }
            FormatError::Malformed(m) => write!(f, "malformed recording: {m}"),
            FormatError::Dropped(n) => {
                write!(
                    f,
                    "the recorder dropped {n} peripheral loads (log too small)"
                )
            }
        }
    }
}

impl std::error::Error for FormatError {}

struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn u32(&mut self) -> Result<u32, FormatError> {
        let b = self
            .data
            .get(self.pos..self.pos + 4)
            .ok_or(FormatError::Truncated)?;
        self.pos += 4;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn bytes(&mut self, n: usize) -> Result<&'a [u8], FormatError> {
        let b = self
            .data
            .get(self.pos..self.pos.checked_add(n).ok_or(FormatError::Truncated)?)
            .ok_or(FormatError::Truncated)?;
        self.pos += n;
        Ok(b)
    }
}

fn words(bytes: &[u8]) -> Vec<u32> {
    bytes
        .chunks_exact(4)
        .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect()
}

fn malformed(what: &str) -> FormatError {
    FormatError::Malformed(what.to_string())
}

fn chunk_name(t: [u8; 4]) -> String {
    String::from_utf8_lossy(&t).trim_end().to_string()
}

/// Parse a recording and check every checksum.
///
/// # Errors
/// [`FormatError`] if the file is not a complete, intact, usable
/// recording.
pub fn parse(data: &[u8]) -> Result<Recording, FormatError> {
    if data.len() < 4 {
        return Err(FormatError::Truncated);
    }
    let body = &data[..data.len() - 4];
    let mut r = Reader { data, pos: 0 };
    if r.u32()? != MAGIC {
        return Err(FormatError::BadMagic);
    }
    let version = r.u32()?;
    if version != VERSION {
        return Err(FormatError::UnsupportedVersion(version));
    }
    let trailer = u32::from_le_bytes([
        data[data.len() - 4],
        data[data.len() - 3],
        data[data.len() - 2],
        data[data.len() - 1],
    ]);
    if crc32(body) != trailer {
        return Err(FormatError::FileChecksum);
    }
    if r.u32()? != 16 || r.u32()? != 4 {
        return Err(malformed("header"));
    }
    let mut payloads = Vec::new();
    for expected in [chunk::TARGET, chunk::SNAPSHOT, chunk::MMIO, chunk::END] {
        let t = r.bytes(4)?;
        let t = [t[0], t[1], t[2], t[3]];
        if t != expected {
            return Err(FormatError::Malformed(format!(
                "expected chunk {}, found {:?}",
                chunk_name(expected),
                String::from_utf8_lossy(&t)
            )));
        }
        let len = r.u32()? as usize;
        if len % 4 != 0 || r.pos + len + 4 > body.len() {
            return Err(FormatError::Malformed(format!(
                "chunk {} length",
                chunk_name(t)
            )));
        }
        let payload = r.bytes(len)?;
        let crc = r.u32()?;
        if crc32(payload) != crc {
            return Err(FormatError::ChunkChecksum(chunk_name(t)));
        }
        payloads.push(payload);
    }
    if r.pos != body.len() {
        return Err(malformed("data after the last chunk"));
    }
    let target = words(payloads[0]);
    if target.len() != 4 {
        return Err(malformed("TGTD size"));
    }
    let snapshot = parse_snapshot(payloads[1])?;
    let (events, dropped) = parse_mmio(payloads[2])?;
    if dropped != 0 {
        return Err(FormatError::Dropped(dropped));
    }
    let end = parse_end(payloads[3])?;
    Ok(Recording {
        target_id: target[0],
        image_crc32: target[1],
        image_start: target[2],
        image_len: target[3],
        snapshot,
        events,
        dropped,
        end,
    })
}

fn parse_snapshot(p: &[u8]) -> Result<Snapshot, FormatError> {
    if p.len() < SNAPSHOT_HEADER_BYTES {
        return Err(malformed("SNAP size"));
    }
    let w = words(&p[..SNAPSHOT_HEADER_BYTES]);
    if w[0] != SNAPSHOT_MAGIC || w[1] != 1 || w[2] as usize != SNAPSHOT_HEADER_BYTES {
        return Err(malformed("snapshot header"));
    }
    let count = w[3] as usize;
    if count > 4 {
        return Err(malformed("snapshot region count"));
    }
    let cpu = CpuState::from_words(&w[4..4 + CPU_WORDS]);
    let s = 4 + CPU_WORDS; // SCS state
    let ipr_start = 4 * (s + 7 + 8);
    let mut shpr = [0u32; 3];
    shpr.copy_from_slice(&w[s + 3..s + 6]);
    let mut iser = [0u32; 8];
    iser.copy_from_slice(&w[s + 7..s + 15]);
    let after_ipr = (ipr_start + 240) / 4; // word index after the IPR bytes
    let mut mpu = [0u32; 17];
    mpu.copy_from_slice(&w[after_ipr + 2..after_ipr + 19]);
    let scs = ScsState {
        vtor: w[s],
        aircr: w[s + 1],
        ccr: w[s + 2],
        shpr,
        shcsr: w[s + 6],
        nvic_iser: iser,
        nvic_ipr: p[ipr_start..ipr_start + 240].to_vec(),
        syst: [w[after_ipr], w[after_ipr + 1]],
        mpu,
        cpacr: w[after_ipr + 19],
    };
    let regions_at = after_ipr + 20;
    let mut offset = SNAPSHOT_HEADER_BYTES;
    let mut regions = Vec::new();
    let mut crc = 0;
    for i in 0..count {
        let address = w[regions_at + 2 * i];
        let len = w[regions_at + 2 * i + 1] as usize;
        let bytes = p
            .get(offset..offset + len)
            .ok_or_else(|| malformed("snapshot region extends past the chunk"))?;
        crc = update(crc, bytes);
        regions.push(Region {
            address,
            bytes: bytes.to_vec(),
        });
        offset += len;
    }
    if offset != p.len() {
        return Err(malformed("snapshot size does not match its regions"));
    }
    if crc != w[regions_at + 8] {
        return Err(FormatError::SnapshotChecksum);
    }
    Ok(Snapshot { cpu, scs, regions })
}

fn parse_mmio(p: &[u8]) -> Result<(Vec<MmioEvent>, u32), FormatError> {
    let w = words(p);
    if w.len() < 2 || (w.len() - 2) % 4 != 0 || (w.len() - 2) / 4 != w[0] as usize {
        return Err(malformed("MMIO size does not match its count"));
    }
    let mut events = Vec::with_capacity(w[0] as usize);
    for e in w[2..].chunks_exact(4) {
        let size = (e[3] & 0xFF) as u8;
        if !matches!(size, 1 | 2 | 4) || e[3] >> 8 != 0 {
            return Err(malformed("MMIO event size"));
        }
        if size < 4 && e[1] >> (8 * u32::from(size)) != 0 {
            return Err(malformed("MMIO value wider than its load"));
        }
        events.push(MmioEvent {
            address: e[0],
            value: e[1],
            pc: e[2],
            size,
        });
    }
    Ok((events, w[1]))
}

fn parse_end(p: &[u8]) -> Result<EndState, FormatError> {
    let w = words(p);
    if w.len() != CPU_WORDS + 2 + 4 || w[CPU_WORDS + 1] > 2 {
        return Err(malformed("END size"));
    }
    let count = w[CPU_WORDS + 1] as usize;
    let regions = (0..count)
        .map(|i| (w[CPU_WORDS + 2 + 2 * i], w[CPU_WORDS + 3 + 2 * i]))
        .collect();
    Ok(EndState {
        cpu: CpuState::from_words(&w[..CPU_WORDS]),
        ram_crc32: w[CPU_WORDS],
        regions,
    })
}

fn push_words(out: &mut Vec<u8>, words: &[u32]) {
    for w in words {
        out.extend_from_slice(&w.to_le_bytes());
    }
}

fn push_chunk(out: &mut Vec<u8>, t: [u8; 4], payload: &[u8]) {
    out.extend_from_slice(&t);
    push_words(out, &[u32::try_from(payload.len()).unwrap_or(u32::MAX)]);
    out.extend_from_slice(payload);
    push_words(out, &[crc32(payload)]);
}

/// Serialize a recording (with fresh checksums).
#[must_use]
pub fn write(rec: &Recording) -> Vec<u8> {
    let len32 = |n: usize| u32::try_from(n).unwrap_or(u32::MAX);
    let mut out = Vec::new();
    push_words(&mut out, &[MAGIC, VERSION, 16, 4]);
    let mut target = Vec::new();
    push_words(
        &mut target,
        &[
            rec.target_id,
            rec.image_crc32,
            rec.image_start,
            rec.image_len,
        ],
    );
    push_chunk(&mut out, chunk::TARGET, &target);

    let s = &rec.snapshot;
    let mut snap = Vec::new();
    push_words(
        &mut snap,
        &[
            SNAPSHOT_MAGIC,
            1,
            len32(SNAPSHOT_HEADER_BYTES),
            len32(s.regions.len()),
        ],
    );
    push_words(&mut snap, &s.cpu.words());
    push_words(
        &mut snap,
        &[
            s.scs.vtor,
            s.scs.aircr,
            s.scs.ccr,
            s.scs.shpr[0],
            s.scs.shpr[1],
            s.scs.shpr[2],
            s.scs.shcsr,
        ],
    );
    push_words(&mut snap, &s.scs.nvic_iser);
    let mut ipr = s.scs.nvic_ipr.clone();
    ipr.resize(240, 0);
    snap.extend_from_slice(&ipr);
    push_words(&mut snap, &s.scs.syst);
    push_words(&mut snap, &s.scs.mpu);
    push_words(&mut snap, &[s.scs.cpacr]);
    let mut crc = 0;
    for i in 0..4 {
        match s.regions.get(i) {
            Some(r) => {
                push_words(&mut snap, &[r.address, len32(r.bytes.len())]);
                crc = update(crc, &r.bytes);
            }
            None => push_words(&mut snap, &[0, 0]),
        }
    }
    push_words(&mut snap, &[crc]);
    for r in &s.regions {
        snap.extend_from_slice(&r.bytes);
    }
    push_chunk(&mut out, chunk::SNAPSHOT, &snap);

    let mut mmio = Vec::new();
    push_words(&mut mmio, &[len32(rec.events.len()), rec.dropped]);
    for e in &rec.events {
        push_words(&mut mmio, &[e.address, e.value, e.pc, u32::from(e.size)]);
    }
    push_chunk(&mut out, chunk::MMIO, &mmio);

    let mut end = Vec::new();
    push_words(&mut end, &rec.end.cpu.words());
    push_words(&mut end, &[rec.end.ram_crc32, len32(rec.end.regions.len())]);
    for i in 0..2 {
        let (a, l) = rec.end.regions.get(i).copied().unwrap_or((0, 0));
        push_words(&mut end, &[a, l]);
    }
    push_chunk(&mut out, chunk::END, &end);
    let file_crc = crc32(&out);
    push_words(&mut out, &[file_crc]);
    out
}
