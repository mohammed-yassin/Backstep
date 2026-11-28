//! The recording format: writing and parsing agree, and every corruption
//! of the bytes is rejected (the integrity half of the self-check; the
//! replay half is tested end to end by scripts/replay-test.py).

use backstep_replay::recording::{
    parse, write, CpuState, EndState, FormatError, FpState, IrqEvent, MmioEvent, Recording, Region,
    ScsState, Snapshot,
};

fn sample() -> Recording {
    let cpu = CpuState {
        r: [0, 0, 0, 0, 4, 5, 6, 7, 8, 9, 10, 11, 0],
        sp: 0x2000_0FF0,
        lr: 0x0000_0123,
        pc: 0x0000_0122,
        xpsr: 0x6100_0000,
        msp: 0x2000_0FF0,
        ..CpuState::default()
    };
    Recording {
        target_id: 1,
        image_crc32: 0xCAFE_F00D,
        image_start: 0,
        image_len: 0x400,
        snapshot: Snapshot {
            cpu,
            scs: ScsState {
                vtor: 0,
                aircr: 0xFA05_0300,
                ccr: 0x200,
                shpr: [1, 2, 3],
                shcsr: 0x1_0000,
                nvic_iser: [1, 0, 0, 0, 0, 0, 0, 0],
                nvic_ipr: (0..240u8).collect(),
                syst: [5, 1000],
                mpu: [7; 17],
                cpacr: 0x00F0_0000,
            },
            regions: vec![
                Region {
                    address: 0x2000_0000,
                    bytes: (0..64u8).collect(),
                },
                Region {
                    address: 0x2000_0FF0,
                    bytes: vec![0xAA; 16],
                },
            ],
        },
        events: (0..5u32)
            .map(|i| MmioEvent {
                address: 0x4000_0004 + 4 * i,
                value: i * 1000,
                pc: 0x200 + 2 * i,
                size: [1, 2, 4, 4, 2][i as usize],
            })
            .collect(),
        dropped: 0,
        irqs: (0..3u32)
            .map(|i| IrqEvent {
                exception: 15 + 9 * i,
                r: [i; 13],
                sp: 0x2000_0F00 - 32 * i,
                lr: 0xFFFF_FFF9,
                pc: 0x300 + 4 * i,
                xpsr: 0x0100_0000 | i,
                mmio_index: 2 * i,
                stack_hash: 0xABCD_0000 + i,
                fpccr: (i != 1).then_some(0xC000_0031 - i),
            })
            .collect(),
        irq_stack_top: 0x2000_1000,
        fp_start: FpState {
            valid: true,
            s: std::array::from_fn(|i| 0x3F80_0000 + u32::try_from(i).unwrap()),
            fpscr: 0x0300_0010,
            fpccr: 0xC000_0000,
            fpcar: 0x2000_0F80,
            fpdscr: 0x0040_0000,
        },
        fp_end: FpState::default(),
        end: EndState {
            cpu,
            ram_crc32: 0x1234_5678,
            regions: vec![(0x2000_0000, 64), (0x2000_0FF0, 16)],
        },
    }
}

#[test]
fn write_then_parse_is_identity() {
    let rec = sample();
    let bytes = write(&rec);
    assert_eq!(parse(&bytes), Ok(rec));
}

#[test]
fn every_single_bit_flip_is_rejected() {
    let bytes = write(&sample());
    for pos in 0..bytes.len() {
        for bit in 0..8 {
            let mut bad = bytes.clone();
            bad[pos] ^= 1 << bit;
            assert!(
                parse(&bad).is_err(),
                "flip of bit {bit} at byte {pos} accepted"
            );
        }
    }
}

#[test]
fn every_truncation_and_extension_is_rejected() {
    let bytes = write(&sample());
    for len in 0..bytes.len() {
        assert!(parse(&bytes[..len]).is_err(), "truncated to {len} accepted");
    }
    let mut longer = bytes.clone();
    longer.push(0);
    assert!(parse(&longer).is_err());
}

#[test]
fn unusable_recordings_say_why() {
    let mut rec = sample();
    rec.dropped = 3;
    assert_eq!(parse(&write(&rec)), Err(FormatError::Dropped(3)));
    let mut bytes = write(&sample());
    bytes[4] = 4; // version
    assert_eq!(parse(&bytes), Err(FormatError::UnsupportedVersion(4)));
    assert_eq!(parse(b"ELF\x7f....."), Err(FormatError::BadMagic));
    // A snapshot whose region CRC does not match (written consistently
    // otherwise) is caught by the snapshot's own checksum.
    let mut rec = sample();
    rec.snapshot.regions[0].bytes[0] ^= 1;
    let mut bytes = write(&rec);
    // Re-insert the CRC of the original contents: the snapshot now lies.
    // Layout: file header (16), TGTD chunk (8 + 16 + 4), SNAP type and
    // length (8), then the snapshot header, whose word 130 is its CRC.
    let snap_start = 16 + 28 + 8;
    let good = write(&sample());
    let snap_crc_at = snap_start + 4 * 130;
    bytes[snap_crc_at..snap_crc_at + 4].copy_from_slice(&good[snap_crc_at..snap_crc_at + 4]);
    // Fix up the chunk and file CRCs so only the snapshot CRC is wrong.
    let snap_len =
        u32::from_le_bytes(bytes[snap_start - 4..snap_start].try_into().unwrap()) as usize;
    let crc = backstep_core::crc32::crc32(&bytes[snap_start..snap_start + snap_len]);
    bytes[snap_start + snap_len..snap_start + snap_len + 4].copy_from_slice(&crc.to_le_bytes());
    let n = bytes.len();
    let file = backstep_core::crc32::crc32(&bytes[..n - 4]);
    bytes[n - 4..].copy_from_slice(&file.to_le_bytes());
    assert_eq!(parse(&bytes), Err(FormatError::SnapshotChecksum));
    // A value wider than its load is malformed.
    let mut rec = sample();
    rec.events[0].value = 0x100; // a 1-byte load
    assert!(matches!(
        parse(&write(&rec)),
        Err(FormatError::Malformed(_))
    ));
    // Every error explains itself.
    for e in [
        FormatError::Truncated,
        FormatError::FileChecksum,
        FormatError::ChunkChecksum("MMIO".into()),
        FormatError::Malformed("x".into()),
    ] {
        assert_ne!(e.to_string(), "");
    }
}

/// The chunks of a well-formed file: (type, payload).
fn chunks(bytes: &[u8]) -> Vec<([u8; 4], Vec<u8>)> {
    let word = |at: usize| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()) as usize;
    let mut out = Vec::new();
    let mut pos = 16;
    for _ in 0..word(12) {
        let len = word(pos + 4);
        out.push((
            bytes[pos..pos + 4].try_into().unwrap(),
            bytes[pos + 8..pos + 8 + len].to_vec(),
        ));
        pos += 12 + len;
    }
    out
}

/// The IRQS chunk with the events of version 2 (20 words, no FPCCR).
fn irqs_v2(payload: &[u8]) -> Vec<u8> {
    let mut out = payload[..12].to_vec();
    for event in payload[12..].chunks(84) {
        out.extend_from_slice(&event[..80]);
    }
    out
}

/// A file from chunks, with valid checksums.
fn signed(version: u32, parts: &[([u8; 4], Vec<u8>)]) -> Vec<u8> {
    let crc32 = backstep_core::crc32::crc32;
    let mut out = Vec::new();
    let count = u32::try_from(parts.len()).unwrap();
    for w in [0x4352_5342u32, version, 16, count] {
        out.extend_from_slice(&w.to_le_bytes());
    }
    for (kind, payload) in parts {
        out.extend_from_slice(kind);
        out.extend_from_slice(&u32::try_from(payload.len()).unwrap().to_le_bytes());
        out.extend_from_slice(payload);
        out.extend_from_slice(&crc32(payload).to_le_bytes());
    }
    let file = crc32(&out);
    out.extend_from_slice(&file.to_le_bytes());
    out
}

#[test]
fn the_signing_helper_reproduces_the_writer() {
    let bytes = write(&sample());
    assert_eq!(signed(3, &chunks(&bytes)), bytes);
}

#[test]
fn version_2_recordings_without_floating_point_state_still_parse() {
    let mut parts = chunks(&write(&sample()));
    assert_eq!(&parts[4].0, b"FPUS");
    parts.remove(4);
    let v3_events = parts[3].1.clone();
    parts[3].1 = irqs_v2(&v3_events);
    let mut expected = sample();
    expected.fp_start = FpState::default();
    expected.fp_end = FpState::default();
    for e in &mut expected.irqs {
        e.fpccr = None;
    }
    assert_eq!(parse(&signed(2, &parts)), Ok(expected));
    parts[3].1 = v3_events;
    // A version 3 file must have FPUS.
    assert!(matches!(
        parse(&signed(3, &parts)),
        Err(FormatError::Malformed(_))
    ));
    // FPUS has exactly two states, with a valid flag of 0 or 1.
    let mut parts = chunks(&write(&sample()));
    parts[4].1.truncate(4 * 37);
    assert!(matches!(
        parse(&signed(3, &parts)),
        Err(FormatError::Malformed(_))
    ));
    let mut parts = chunks(&write(&sample()));
    parts[4].1[0] = 2;
    assert!(matches!(
        parse(&signed(3, &parts)),
        Err(FormatError::Malformed(_))
    ));
}

#[test]
fn version_1_recordings_without_interrupts_still_parse() {
    let mut parts = chunks(&write(&sample()));
    assert_eq!(&parts[3].0, b"IRQS");
    parts.remove(4); // FPUS
    parts.remove(3);
    let mut expected = sample();
    expected.irqs.clear();
    expected.irq_stack_top = 0;
    expected.fp_start = FpState::default();
    expected.fp_end = FpState::default();
    assert_eq!(parse(&signed(1, &parts)), Ok(expected));
    // A version 2 file must have its IRQS chunk, and a version 1 file must
    // not have one.
    assert!(matches!(
        parse(&signed(2, &parts)),
        Err(FormatError::Malformed(_))
    ));
    let mut v2 = chunks(&write(&sample()));
    v2.remove(4);
    assert!(matches!(
        parse(&signed(1, &v2)),
        Err(FormatError::Malformed(_))
    ));
}

#[test]
fn interrupt_recordings_that_lost_events_are_refused() {
    let mut parts = chunks(&write(&sample()));
    parts[3].1[4..8].copy_from_slice(&7u32.to_le_bytes()); // dropped
    assert_eq!(
        parse(&signed(3, &parts)),
        Err(FormatError::DroppedInterrupts(7))
    );
    assert!(FormatError::DroppedInterrupts(7).to_string().contains('7'));
    // An event count that disagrees with the chunk size is malformed.
    let mut parts = chunks(&write(&sample()));
    parts[3].1[0..4].copy_from_slice(&4u32.to_le_bytes());
    assert!(matches!(
        parse(&signed(3, &parts)),
        Err(FormatError::Malformed(_))
    ));
}
