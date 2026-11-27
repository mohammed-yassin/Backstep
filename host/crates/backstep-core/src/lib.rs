//! Core library for Backstep.
//!
//! Backstep records the non-deterministic inputs of a microcontroller
//! program (peripheral reads, interrupt arrival points, DMA transfers) and
//! replays the exact execution on a PC, including stepping backwards.
//!
//! Milestone M0 provides the shared CRC-32 used to checksum CPU and memory
//! state, and the version information shared with the on-device recorder.
//! The recording format, emulator and replay engine arrive in later
//! milestones; see `docs/ROADMAP.md`.

pub mod crc32;

/// Version of the host tools, taken from the workspace manifest.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The Cortex-M targets Backstep can currently build and test firmware for.
pub const SUPPORTED_TARGETS: &[Target] = &[
    Target {
        name: "qemu-mps2-an386",
        description: "QEMU Arm MPS2 AN386 (Cortex-M4) - reference target for automated tests",
    },
    Target {
        name: "nucleo-f411re",
        description: "ST NUCLEO-F411RE (STM32F411RE, Cortex-M4F @ 16 MHz HSI)",
    },
];

/// A firmware target known to Backstep.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Target {
    /// Short identifier, matching the directory under `firmware/targets/`.
    pub name: &'static str,
    /// Human-readable description.
    pub description: &'static str,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_matches_recorder_header() {
        // The C recorder and the host tools are versioned together.
        let header = include_str!("../../../../recorder/include/backstep.h");
        let line = header
            .lines()
            .find(|l| l.starts_with("#define BACKSTEP_VERSION_STRING"))
            .expect("version define present in backstep.h");
        let quoted = line.split('"').nth(1).expect("quoted version string");
        assert_eq!(quoted, VERSION);
    }

    #[test]
    fn target_names_match_firmware_directories() {
        for target in SUPPORTED_TARGETS {
            let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../../firmware/targets")
                .join(target.name);
            assert!(
                dir.is_dir(),
                "missing firmware directory for {}",
                target.name
            );
        }
    }
}
