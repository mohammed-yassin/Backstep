//! The recording format: writing and parsing agree, and every corruption
//! of the bytes is rejected (the integrity half of the self-check; the
//! replay half is tested end to end by scripts/replay-test.py).

use backstep_replay::recording::{
    parse, write, CpuState, EndState, FormatError, MmioEvent, Recording, Region, ScsState, Snapshot,
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
    bytes[4] = 2; // version
    assert_eq!(parse(&bytes), Err(FormatError::UnsupportedVersion(2)));
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
