//! What replay needs to know about a target: its memory map and the core
//! options of its chip. Identified by the number the recorder stores
//! (`BS_TARGET_*` in `backstep_record.h`).

/// A target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Target {
    /// Recorder target id.
    pub id: u32,
    /// Name (as in `backstep info`).
    pub name: &'static str,
    /// Memory regions: (base, size, writable).
    pub memory: &'static [(u32, u32, bool)],
    /// Address 0 aliases flash: (flash base, size) copied to 0 after loading.
    pub boot_alias: Option<(u32, u32)>,
    /// External interrupts.
    pub num_irqs: usize,
    /// Implemented priority bits.
    pub priority_mask: u8,
}

/// QEMU `mps2-an386`.
pub const QEMU_MPS2_AN386: Target = Target {
    id: 1,
    name: "qemu-mps2-an386",
    memory: &[
        (0x0000_0000, 0x0040_0000, true),
        (0x2000_0000, 0x0040_0000, true),
    ],
    boot_alias: None,
    num_irqs: 32,
    priority_mask: 0xFF,
};

/// NUCLEO-F411RE (STM32F411RE).
pub const NUCLEO_F411RE: Target = Target {
    id: 2,
    name: "nucleo-f411re",
    memory: &[
        (0x0000_0000, 0x0008_0000, false), // boot alias of flash
        (0x0800_0000, 0x0008_0000, false), // flash
        (0x2000_0000, 0x0002_0000, true),  // SRAM
    ],
    boot_alias: Some((0x0800_0000, 0x0008_0000)),
    num_irqs: 86,
    priority_mask: 0xF0,
};

/// All known targets.
pub const TARGETS: &[Target] = &[QEMU_MPS2_AN386, NUCLEO_F411RE];

/// The target with recorder id `id`.
#[must_use]
pub fn by_id(id: u32) -> Option<Target> {
    TARGETS.iter().copied().find(|t| t.id == id)
}
