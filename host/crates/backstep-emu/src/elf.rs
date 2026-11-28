//! Minimal ELF32 little-endian loader: copies `PT_LOAD` segments into
//! memory at their physical (load) addresses, as a debug probe would when
//! flashing, and finds symbols.

use crate::memory::Memory;

/// A loading error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ElfError(pub String);

impl std::fmt::Display for ElfError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ELF: {}", self.0)
    }
}

impl std::error::Error for ElfError {}

fn u16_at(d: &[u8], o: usize) -> Result<u16, ElfError> {
    d.get(o..o + 2)
        .map(|b| u16::from_le_bytes([b[0], b[1]]))
        .ok_or_else(|| ElfError(format!("truncated at {o}")))
}

fn u32_at(d: &[u8], o: usize) -> Result<u32, ElfError> {
    d.get(o..o + 4)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .ok_or_else(|| ElfError(format!("truncated at {o}")))
}

/// A parsed ELF file.
#[derive(Debug, Clone)]
pub struct Elf {
    data: Vec<u8>,
    /// Entry point.
    pub entry: u32,
}

/// One loadable segment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Segment {
    /// Load address.
    pub paddr: u32,
    /// Offset in the file.
    pub offset: u32,
    /// Bytes present in the file.
    pub filesz: u32,
}

impl Elf {
    /// Parse ELF32, little-endian, ARM.
    ///
    /// # Errors
    /// If the file is not such an ELF.
    pub fn parse(data: Vec<u8>) -> Result<Self, ElfError> {
        if data.get(0..4) != Some(b"\x7fELF") {
            return Err(ElfError("not an ELF file".into()));
        }
        if data[4] != 1 || data[5] != 1 {
            return Err(ElfError("not 32-bit little-endian".into()));
        }
        if u16_at(&data, 18)? != 40 {
            return Err(ElfError("not an ARM file".into()));
        }
        let entry = u32_at(&data, 24)?;
        Ok(Elf { data, entry })
    }

    /// Loadable segments with file content.
    ///
    /// # Errors
    /// On a malformed program header table.
    pub fn segments(&self) -> Result<Vec<Segment>, ElfError> {
        let phoff = u32_at(&self.data, 28)? as usize;
        let phentsize = usize::from(u16_at(&self.data, 42)?);
        let phnum = usize::from(u16_at(&self.data, 44)?);
        let mut out = Vec::new();
        for i in 0..phnum {
            let o = phoff + i * phentsize;
            if u32_at(&self.data, o)? != 1 {
                continue; // not PT_LOAD
            }
            let seg = Segment {
                offset: u32_at(&self.data, o + 4)?,
                paddr: u32_at(&self.data, o + 12)?,
                filesz: u32_at(&self.data, o + 16)?,
            };
            if seg.filesz > 0 {
                out.push(seg);
            }
        }
        Ok(out)
    }

    /// Copy every loadable segment into memory at its load address.
    ///
    /// # Errors
    /// If a segment does not fit in the memory's regions.
    pub fn load_into(&self, memory: &mut Memory) -> Result<(), ElfError> {
        for s in self.segments()? {
            let bytes = self
                .data
                .get(s.offset as usize..(s.offset + s.filesz) as usize)
                .ok_or_else(|| ElfError("segment outside file".into()))?;
            memory
                .load(s.paddr, bytes)
                .map_err(|f| ElfError(format!("segment byte at {:#x} not in memory", f.address)))?;
        }
        Ok(())
    }

    /// The contents of the section called `name`, if present (debug
    /// information, for example).
    #[must_use]
    pub fn section(&self, name: &str) -> Option<&[u8]> {
        let shoff = u32_at(&self.data, 32).ok()? as usize;
        let shentsize = usize::from(u16_at(&self.data, 46).ok()?);
        let shnum = usize::from(u16_at(&self.data, 48).ok()?);
        let shstrndx = usize::from(u16_at(&self.data, 50).ok()?);
        let names = u32_at(&self.data, shoff + shstrndx * shentsize + 16).ok()? as usize;
        for i in 0..shnum {
            let sh = shoff + i * shentsize;
            let name_off = u32_at(&self.data, sh).ok()? as usize;
            let start = names + name_off;
            let end = start + self.data.get(start..)?.iter().position(|&b| b == 0)?;
            if &self.data[start..end] != name.as_bytes() {
                continue;
            }
            if u32_at(&self.data, sh + 4).ok()? == 8 {
                return Some(&[]); // SHT_NOBITS
            }
            let off = u32_at(&self.data, sh + 16).ok()? as usize;
            let size = u32_at(&self.data, sh + 20).ok()? as usize;
            return self.data.get(off..off + size);
        }
        None
    }

    /// Function symbols from `.symtab`: (name, address without the Thumb
    /// bit, size), sorted by address.
    #[must_use]
    pub fn functions(&self) -> Vec<(String, u32, u32)> {
        let mut out = Vec::new();
        let Some(shoff) = u32_at(&self.data, 32).ok().map(|v| v as usize) else {
            return out;
        };
        let (Ok(shentsize), Ok(shnum)) = (u16_at(&self.data, 46), u16_at(&self.data, 48)) else {
            return out;
        };
        let (shentsize, shnum) = (usize::from(shentsize), usize::from(shnum));
        for i in 0..shnum {
            let sh = shoff + i * shentsize;
            if u32_at(&self.data, sh + 4).ok() != Some(2) {
                continue; // SHT_SYMTAB
            }
            let (Ok(off), Ok(size), Ok(link)) = (
                u32_at(&self.data, sh + 16),
                u32_at(&self.data, sh + 20),
                u32_at(&self.data, sh + 24),
            ) else {
                continue;
            };
            let Ok(strtab) = u32_at(&self.data, shoff + link as usize * shentsize + 16) else {
                continue;
            };
            for s in (off as usize..(off + size) as usize).step_by(16) {
                let info = self.data.get(s + 12).copied().unwrap_or(0);
                if info & 0xF != 2 {
                    continue; // STT_FUNC
                }
                let (Ok(name_off), Ok(value), Ok(sz)) = (
                    u32_at(&self.data, s),
                    u32_at(&self.data, s + 4),
                    u32_at(&self.data, s + 8),
                ) else {
                    continue;
                };
                let start = strtab as usize + name_off as usize;
                let Some(len) = self
                    .data
                    .get(start..)
                    .and_then(|d| d.iter().position(|&b| b == 0))
                else {
                    continue;
                };
                let name = String::from_utf8_lossy(&self.data[start..start + len]).into_owned();
                out.push((name, value & !1, sz));
            }
        }
        out.sort_by_key(|f| f.1);
        out
    }

    /// Value of a symbol from `.symtab`, if present.
    #[must_use]
    pub fn symbol(&self, name: &str) -> Option<u32> {
        let shoff = u32_at(&self.data, 32).ok()? as usize;
        let shentsize = usize::from(u16_at(&self.data, 46).ok()?);
        let shnum = usize::from(u16_at(&self.data, 48).ok()?);
        for i in 0..shnum {
            let sh = shoff + i * shentsize;
            if u32_at(&self.data, sh + 4).ok()? != 2 {
                continue; // SHT_SYMTAB
            }
            let off = u32_at(&self.data, sh + 16).ok()? as usize;
            let size = u32_at(&self.data, sh + 20).ok()? as usize;
            let link = u32_at(&self.data, sh + 24).ok()? as usize;
            let strtab = u32_at(&self.data, shoff + link * shentsize + 16).ok()? as usize;
            for s in (off..off + size).step_by(16) {
                let name_off = u32_at(&self.data, s).ok()? as usize;
                let start = strtab + name_off;
                let end = start + self.data[start..].iter().position(|&b| b == 0)?;
                if &self.data[start..end] == name.as_bytes() {
                    return u32_at(&self.data, s + 4).ok();
                }
            }
        }
        None
    }
}
