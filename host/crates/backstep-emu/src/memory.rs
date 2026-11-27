//! Memory: RAM/flash regions owned by the emulator, everything else
//! forwarded to a [`Bus`] (peripherals, which replay answers from the
//! recording).

/// A failed access.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fault {
    /// Faulting address.
    pub address: u32,
    /// Access size in bytes.
    pub size: u8,
    /// True for writes.
    pub write: bool,
}

/// Accesses that fall outside the emulator's own memory regions.
pub trait Bus {
    /// Read `size` bytes (1, 2 or 4) at `address`.
    ///
    /// # Errors
    /// Returns a [`Fault`] if nothing answers at that address.
    fn read(&mut self, address: u32, size: u8) -> Result<u32, Fault>;
    /// Write `size` bytes (1, 2 or 4) at `address`.
    ///
    /// # Errors
    /// Returns a [`Fault`] if nothing answers at that address.
    fn write(&mut self, address: u32, size: u8, value: u32) -> Result<(), Fault>;
}

/// A bus with nothing on it: every access faults.
#[derive(Debug, Default)]
pub struct NoBus;

impl Bus for NoBus {
    fn read(&mut self, address: u32, size: u8) -> Result<u32, Fault> {
        Err(Fault {
            address,
            size,
            write: false,
        })
    }
    fn write(&mut self, address: u32, size: u8, _value: u32) -> Result<(), Fault> {
        Err(Fault {
            address,
            size,
            write: true,
        })
    }
}

#[derive(Debug, Clone)]
struct Region {
    base: u32,
    data: Vec<u8>,
    writable: bool,
}

/// Little-endian memory made of non-overlapping regions.
#[derive(Debug, Clone, Default)]
pub struct Memory {
    regions: Vec<Region>,
}

impl Memory {
    /// Empty memory.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a zero-filled region.
    ///
    /// # Panics
    /// If it overlaps an existing region or wraps past 4 GiB.
    pub fn add_region(&mut self, base: u32, size: u32, writable: bool) {
        let end = u64::from(base) + u64::from(size);
        assert!(end <= 1 << 32, "region wraps past 4 GiB");
        for r in &self.regions {
            let r_end = u64::from(r.base) + r.data.len() as u64;
            assert!(
                end <= u64::from(r.base) || u64::from(base) >= r_end,
                "regions overlap"
            );
        }
        self.regions.push(Region {
            base,
            data: vec![0; size as usize],
            writable,
        });
    }

    fn find(&self, address: u32, size: u8) -> Option<(usize, usize)> {
        self.regions.iter().enumerate().find_map(|(i, r)| {
            let off = address.wrapping_sub(r.base) as usize;
            (address >= r.base && off + size as usize <= r.data.len()).then_some((i, off))
        })
    }

    /// Copy bytes into a region (loading, independent of `writable`).
    ///
    /// # Errors
    /// If any byte is outside every region.
    pub fn load(&mut self, address: u32, bytes: &[u8]) -> Result<(), Fault> {
        for (i, b) in bytes.iter().enumerate() {
            let a = address.wrapping_add(i as u32);
            let (r, off) = self.find(a, 1).ok_or(Fault {
                address: a,
                size: 1,
                write: true,
            })?;
            self.regions[r].data[off] = *b;
        }
        Ok(())
    }

    /// Read from a region. `None` if the address is not in one.
    #[must_use]
    pub fn read(&self, address: u32, size: u8) -> Option<u32> {
        let (r, off) = self.find(address, size)?;
        let d = &self.regions[r].data[off..off + size as usize];
        Some(
            d.iter()
                .rev()
                .fold(0u32, |acc, b| (acc << 8) | u32::from(*b)),
        )
    }

    /// Write to a region. `None` if not in one; `Some(false)` if read-only.
    pub fn write(&mut self, address: u32, size: u8, value: u32) -> Option<bool> {
        let (r, off) = self.find(address, size)?;
        if !self.regions[r].writable {
            return Some(false);
        }
        for i in 0..size as usize {
            self.regions[r].data[off + i] = (value >> (8 * i)) as u8;
        }
        Some(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn little_endian_and_bounds() {
        let mut m = Memory::new();
        m.add_region(0x2000_0000, 16, true);
        assert_eq!(m.write(0x2000_0000, 4, 0x1122_3344), Some(true));
        assert_eq!(m.read(0x2000_0000, 1), Some(0x44));
        assert_eq!(m.read(0x2000_0001, 2), Some(0x2233));
        assert_eq!(m.read(0x2000_0000, 4), Some(0x1122_3344));
        assert_eq!(m.read(0x2000_000E, 4), None); // straddles the end
        assert_eq!(m.read(0x1FFF_FFFF, 1), None);
        m.add_region(0, 4, false);
        assert_eq!(m.write(0, 4, 1), Some(false));
        m.load(0, &[1, 2, 3, 4]).unwrap();
        assert_eq!(m.read(0, 4), Some(0x0403_0201));
    }

    #[test]
    #[should_panic(expected = "regions overlap")]
    fn overlapping_regions_rejected() {
        let mut m = Memory::new();
        m.add_region(0x100, 0x100, true);
        m.add_region(0x1F0, 0x100, true);
    }
}
