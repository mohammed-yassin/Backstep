//! Memory: RAM/flash regions owned by the emulator, everything else
//! forwarded to a [`Bus`] (peripherals, which replay answers from the
//! recording).

use std::cell::Cell;
use std::rc::Rc;

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

/// Bytes per page. Memory is stored in shared pages, grouped in shared
/// blocks of [`BLOCK_PAGES`] pages, each copied on the first write after a
/// clone. Cloning a whole machine (a replay checkpoint) then costs one
/// reference count per 64 KiB, and each page written since costs 1 KiB.
pub const PAGE_BYTES: usize = 1024;
/// Pages per block.
pub const BLOCK_PAGES: usize = 64;
const PAGE_MASK: usize = PAGE_BYTES - 1;
const BLOCK_BYTES: usize = PAGE_BYTES * BLOCK_PAGES;

type Page = Rc<[u8; PAGE_BYTES]>;
type Block = Rc<[Page; BLOCK_PAGES]>;

#[derive(Debug, Clone)]
struct Region {
    base: u32,
    len: usize,
    blocks: Vec<Block>,
    writable: bool,
}

impl Region {
    fn page(&self, off: usize) -> &[u8; PAGE_BYTES] {
        &self.blocks[off / BLOCK_BYTES][(off % BLOCK_BYTES) / PAGE_BYTES]
    }

    fn byte(&self, off: usize) -> u8 {
        self.page(off)[off & PAGE_MASK]
    }

    fn set_byte(&mut self, off: usize, value: u8) {
        let block = Rc::make_mut(&mut self.blocks[off / BLOCK_BYTES]);
        Rc::make_mut(&mut block[(off % BLOCK_BYTES) / PAGE_BYTES])[off & PAGE_MASK] = value;
    }
}

/// Addresses watched for writes or reads (debugger watchpoints).
#[derive(Debug, Clone, Default)]
struct Watch {
    /// `[start, end)` ranges and whether reads count too.
    ranges: Vec<(u32, u32, bool)>,
    /// The first watched access since the last [`Memory::take_watch_hit`].
    hit: Cell<Option<WatchHit>>,
}

/// A watched access.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WatchHit {
    /// Address of the access.
    pub address: u32,
    /// Size in bytes.
    pub size: u8,
    /// True for a write.
    pub write: bool,
}

/// Little-endian memory made of non-overlapping regions.
#[derive(Debug, Clone, Default)]
pub struct Memory {
    regions: Vec<Region>,
    watch: Watch,
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
            let r_end = u64::from(r.base) + r.len as u64;
            assert!(
                end <= u64::from(r.base) || u64::from(base) >= r_end,
                "regions overlap"
            );
        }
        let zero: Page = Rc::new([0; PAGE_BYTES]);
        let block: Block = Rc::new(std::array::from_fn(|_| Rc::clone(&zero)));
        self.regions.push(Region {
            base,
            len: size as usize,
            blocks: vec![block; (size as usize).div_ceil(BLOCK_BYTES)],
            writable,
        });
    }

    fn find(&self, address: u32, size: u8) -> Option<(usize, usize)> {
        self.regions.iter().enumerate().find_map(|(i, r)| {
            let off = address.wrapping_sub(r.base) as usize;
            (address >= r.base && off + size as usize <= r.len).then_some((i, off))
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
            self.regions[r].set_byte(off, *b);
        }
        Ok(())
    }

    /// Read from a region. `None` if the address is not in one.
    #[must_use]
    pub fn read(&self, address: u32, size: u8) -> Option<u32> {
        let (r, off) = self.find(address, size)?;
        self.note(address, size, false);
        let region = &self.regions[r];
        let n = size as usize;
        let value = if (off & PAGE_MASK) + n <= PAGE_BYTES {
            let page = &region.page(off)[off & PAGE_MASK..(off & PAGE_MASK) + n];
            page.iter()
                .rev()
                .fold(0u32, |acc, b| (acc << 8) | u32::from(*b))
        } else {
            (0..n)
                .rev()
                .fold(0u32, |acc, i| (acc << 8) | u32::from(region.byte(off + i)))
        };
        Some(value)
    }

    /// Read without counting as a watched access (a debugger looking).
    #[must_use]
    pub fn peek(&self, address: u32, size: u8) -> Option<u32> {
        let hit = self.watch.hit.get();
        let value = self.read(address, size);
        self.watch.hit.set(hit);
        value
    }

    /// Write to a region. `None` if not in one; `Some(false)` if read-only.
    pub fn write(&mut self, address: u32, size: u8, value: u32) -> Option<bool> {
        let (r, off) = self.find(address, size)?;
        if !self.regions[r].writable {
            return Some(false);
        }
        self.note(address, size, true);
        for i in 0..size as usize {
            self.regions[r].set_byte(off + i, (value >> (8 * i)) as u8);
        }
        Some(true)
    }

    /// Watch `[address, address + len)` for writes, and for reads too if
    /// `reads`.
    pub fn watch(&mut self, address: u32, len: u32, reads: bool) {
        self.watch
            .ranges
            .push((address, address.saturating_add(len), reads));
    }

    /// Stop watching a range given to [`Memory::watch`].
    pub fn unwatch(&mut self, address: u32, len: u32, reads: bool) {
        let range = (address, address.saturating_add(len), reads);
        if let Some(i) = self.watch.ranges.iter().position(|r| *r == range) {
            self.watch.ranges.remove(i);
        }
    }

    /// The first watched access since the last call, and forget it.
    pub fn take_watch_hit(&mut self) -> Option<WatchHit> {
        self.watch.hit.take()
    }

    fn note(&self, address: u32, size: u8, write: bool) {
        if self.watch.ranges.is_empty() || self.watch.hit.get().is_some() {
            return;
        }
        let end = u64::from(address) + u64::from(size);
        if self.watch.ranges.iter().any(|&(a, b, reads)| {
            (write || reads) && u64::from(address) < u64::from(b) && end > u64::from(a)
        }) {
            self.watch.hit.set(Some(WatchHit {
                address,
                size,
                write,
            }));
        }
    }

    /// How many pages `self` no longer shares with `other` (a clone of it,
    /// or of a common ancestor): the memory a checkpoint costs.
    #[must_use]
    pub fn pages_not_shared_with(&self, other: &Memory) -> usize {
        let mut n = 0;
        for (a, b) in self.regions.iter().zip(&other.regions) {
            for (x, y) in a.blocks.iter().zip(&b.blocks) {
                if !Rc::ptr_eq(x, y) {
                    n += x
                        .iter()
                        .zip(y.iter())
                        .filter(|(p, q)| !Rc::ptr_eq(p, q))
                        .count();
                }
            }
        }
        n
    }

    /// Write without counting as a watched access (the replay engine
    /// reproducing what the recorder's own code left in memory).
    pub fn poke(&mut self, address: u32, size: u8, value: u32) -> Option<bool> {
        let hit = self.watch.hit.get();
        let done = self.write(address, size, value);
        self.watch.hit.set(hit);
        done
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
    fn accesses_across_page_and_block_boundaries() {
        let mut m = Memory::new();
        m.add_region(0x2000_0000, 3 * BLOCK_BYTES as u32 + 10, true);
        for edge in [PAGE_BYTES, BLOCK_BYTES, 2 * BLOCK_BYTES, 3 * BLOCK_BYTES] {
            let a = 0x2000_0000 + edge as u32 - 2;
            assert_eq!(m.write(a, 4, 0xA1B2_C3D4), Some(true), "{edge}");
            assert_eq!(m.read(a, 4), Some(0xA1B2_C3D4), "{edge}");
            assert_eq!(m.read(a + 1, 2), Some(0xB2C3), "{edge}");
            assert_eq!(m.read(a + 2, 1), Some(0xB2), "{edge}");
        }
        // The last partial page ends exactly at the region's end.
        let end = 0x2000_0000 + 3 * BLOCK_BYTES as u32 + 10;
        assert_eq!(m.read(end - 4, 4), Some(0));
        assert_eq!(m.read(end - 3, 4), None);
        assert_eq!(m.write(end - 2, 4, 1), None);
    }

    #[test]
    fn clones_share_pages_until_written() {
        let mut a = Memory::new();
        a.add_region(0x2000_0000, 0x40_0000, true); // 4 MiB
        a.write(0x2000_0000, 4, 1);
        let b = a.clone();
        assert_eq!(a.pages_not_shared_with(&b), 0);
        a.write(0x2000_0004, 4, 2); // same page
        a.write(0x2030_0000, 4, 3); // another block
        assert_eq!(a.pages_not_shared_with(&b), 2);
        assert_eq!(
            b.read(0x2000_0004, 4),
            Some(0),
            "the clone keeps its contents"
        );
        assert_eq!(b.read(0x2030_0000, 4), Some(0));
        assert_eq!(a.read(0x2030_0000, 4), Some(3));
        assert_eq!(b.read(0x2000_0000, 4), Some(1));
    }

    #[test]
    fn watched_accesses_are_reported_once() {
        let mut m = Memory::new();
        m.add_region(0x2000_0000, 0x100, true);
        m.watch(0x2000_0010, 4, false);
        m.write(0x2000_000C, 4, 7); // just below
        let _ = m.read(0x2000_0010, 4); // a read, not watched for reads
        assert_eq!(m.take_watch_hit(), None);
        m.write(0x2000_0013, 1, 7); // the last byte
        m.write(0x2000_0010, 4, 8); // a second hit is not reported
        assert_eq!(
            m.take_watch_hit(),
            Some(WatchHit {
                address: 0x2000_0013,
                size: 1,
                write: true
            })
        );
        assert_eq!(m.take_watch_hit(), None);
        m.write(0x2000_000E, 4, 0); // overlaps the start
        assert!(m.take_watch_hit().is_some());
        // Debugger and replay-engine accesses are not the program's.
        assert_eq!(m.peek(0x2000_0010, 4), Some(0x0000_0000));
        m.poke(0x2000_0010, 4, 5);
        assert_eq!(m.take_watch_hit(), None);
        // Read watches.
        m.watch(0x2000_0080, 8, true);
        let _ = m.read(0x2000_0084, 2);
        assert_eq!(
            m.take_watch_hit(),
            Some(WatchHit {
                address: 0x2000_0084,
                size: 2,
                write: false
            })
        );
        m.unwatch(0x2000_0080, 8, true);
        m.unwatch(0x2000_0010, 4, false);
        m.write(0x2000_0010, 4, 1);
        let _ = m.read(0x2000_0084, 2);
        assert_eq!(m.take_watch_hit(), None);
    }

    #[test]
    #[should_panic(expected = "regions overlap")]
    fn overlapping_regions_rejected() {
        let mut m = Memory::new();
        m.add_region(0x100, 0x100, true);
        m.add_region(0x1F0, 0x100, true);
    }
}
