//! CRC-32 (IEEE 802.3 / ISO-HDLC), identical to `bs_crc32` in the recorder.
//!
//! Recordings carry checksums of CPU and memory state computed on the
//! device. The replay engine recomputes them with this function; any
//! difference means the replay has diverged from what really happened, and
//! Backstep reports it instead of showing a wrong history.

/// Nibble lookup table for polynomial `0xEDB8_8320` (reflected).
const NIBBLE_TABLE: [u32; 16] = build_nibble_table();

const fn build_nibble_table() -> [u32; 16] {
    let mut table = [0u32; 16];
    let mut i: u32 = 0;
    while i < 16 {
        let mut crc = i;
        let mut bit = 0;
        while bit < 4 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
            bit += 1;
        }
        table[i as usize] = crc;
        i += 1;
    }
    table
}

/// Continue a CRC-32 over `data`. Start with `crc = 0`; feeding chunks gives
/// the same result as [`crc32`] over the concatenation.
#[must_use]
pub fn update(crc: u32, data: &[u8]) -> u32 {
    let mut crc = !crc;
    for &byte in data {
        crc ^= u32::from(byte);
        crc = (crc >> 4) ^ NIBBLE_TABLE[(crc & 0xF) as usize];
        crc = (crc >> 4) ^ NIBBLE_TABLE[(crc & 0xF) as usize];
    }
    !crc
}

/// CRC-32 of `data`. `crc32(b"123456789") == 0xCBF4_3926`.
#[must_use]
pub fn crc32(data: &[u8]) -> u32 {
    update(0, data)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Same vectors as `recorder/tests/test_recorder.c`.
    const VECTORS: &[(&[u8], u32)] = &[
        (b"", 0x0000_0000),
        (b"a", 0xE8B7_BE43),
        (b"abc", 0x3524_41C2),
        (b"123456789", 0xCBF4_3926),
        (b"message digest", 0x2015_9D7F),
        (b"The quick brown fox jumps over the lazy dog", 0x414F_A339),
    ];

    /// Straightforward bitwise reference implementation.
    fn reference(data: &[u8]) -> u32 {
        let mut crc = 0xFFFF_FFFFu32;
        for &byte in data {
            crc ^= u32::from(byte);
            for _ in 0..8 {
                crc = if crc & 1 != 0 {
                    (crc >> 1) ^ 0xEDB8_8320
                } else {
                    crc >> 1
                };
            }
        }
        !crc
    }

    #[test]
    fn standard_vectors() {
        for &(input, expected) in VECTORS {
            assert_eq!(crc32(input), expected, "input {input:?}");
        }
    }

    #[test]
    fn table_matches_c_recorder() {
        // Copied from recorder/src/backstep.c; must stay identical.
        const C_TABLE: [u32; 16] = [
            0x0000_0000,
            0x1DB7_1064,
            0x3B6E_20C8,
            0x26D9_30AC,
            0x76DC_4190,
            0x6B6B_51F4,
            0x4DB2_6158,
            0x5005_713C,
            0xEDB8_8320,
            0xF00F_9344,
            0xD6D6_A3E8,
            0xCB61_B38C,
            0x9B64_C2B0,
            0x86D3_D2D4,
            0xA00A_E278,
            0xBDBD_F21C,
        ];
        assert_eq!(NIBBLE_TABLE, C_TABLE);
    }

    #[test]
    fn all_byte_values() {
        let bytes: Vec<u8> = (0..=255).collect();
        assert_eq!(crc32(&bytes), 0x2905_8C73);
        assert_eq!(crc32(&bytes), reference(&bytes));
    }

    #[test]
    fn incremental_matches_one_shot_at_every_split() {
        let msg = b"The quick brown fox jumps over the lazy dog";
        for split in 0..=msg.len() {
            let (a, b) = msg.split_at(split);
            assert_eq!(update(update(0, a), b), 0x414F_A339, "split at {split}");
        }
    }

    #[test]
    fn matches_reference_on_pseudo_random_inputs() {
        // Deterministic xorshift so the test needs no dependencies.
        let mut state = 0x1234_5678u32;
        for len in 0..512 {
            let data: Vec<u8> = (0..len)
                .map(|_| {
                    state ^= state << 13;
                    state ^= state >> 17;
                    state ^= state << 5;
                    state.to_le_bytes()[0]
                })
                .collect();
            assert_eq!(crc32(&data), reference(&data), "length {len}");
        }
    }
}
