//! Unit tests for behaviour the QEMU differential test cannot reach:
//! stop conditions, UNPREDICTABLE encodings, and state rollback. (Normal
//! instruction semantics are verified against QEMU by
//! `scripts/emu-difftest.py`.)

use backstep_emu::elf::Elf;
use backstep_emu::memory::NoBus;
use backstep_emu::{Cpu, Memory, Stop};

const CODE: u32 = 0x100;

/// A CPU with the halfwords at CODE, SP in RAM, flags clear.
fn cpu_with(code: &[u16]) -> Cpu {
    let mut m = Memory::new();
    m.add_region(0, 0x1000, true);
    m.add_region(0x2000_0000, 0x1000, true);
    let bytes: Vec<u8> = code.iter().flat_map(|h| h.to_le_bytes()).collect();
    m.load(CODE, &bytes).unwrap();
    m.load(0, &0x2000_1000u32.to_le_bytes()).unwrap();
    m.load(4, &(CODE | 1).to_le_bytes()).unwrap();
    let mut cpu = Cpu::new(m);
    cpu.reset(0).unwrap();
    cpu.exceptions = false; // these tests check where execution stops
    cpu
}

fn run(cpu: &mut Cpu, steps: usize) {
    for _ in 0..steps {
        cpu.step(&mut NoBus).unwrap();
    }
}

#[test]
fn reset_reads_the_vector_table() {
    let cpu = cpu_with(&[0xBF00]);
    assert_eq!(cpu.r[13], 0x2000_1000);
    assert_eq!(cpu.r[15], CODE);
    assert_eq!(cpu.r[14], 0xFFFF_FFFF);
    assert_eq!(cpu.xpsr() & 0x0100_0000, 0x0100_0000, "Thumb bit");
}

#[test]
fn it_block_skips_failed_instructions_and_advances() {
    // movs r0,#0 ; ite eq ; moveq r1,#1 (movs encoding, no flags in IT) ;
    // movne r2,#1
    let mut cpu = cpu_with(&[0x2000, 0xBF0C, 0x2101, 0x2201]);
    run(&mut cpu, 2);
    assert_ne!(cpu.it, 0, "inside the IT block");
    run(&mut cpu, 2);
    assert_eq!(
        (cpu.r[1], cpu.r[2]),
        (1, 0),
        "then-branch ran, else skipped"
    );
    assert_eq!(cpu.it, 0, "IT block finished");
    assert!(cpu.flags.z, "flags inside IT are not changed by MOV");
}

#[test]
fn unpredictable_and_undefined_encodings_stop() {
    for (code, what) in [
        (vec![0xDE00], "UDF"),
        (vec![0xBF08, 0xBF08], "IT inside IT"),
        (vec![0xBF04, 0xE000, 0xBF00], "B not last in IT"),
        (vec![0xBC00], "POP with empty list"),
        (vec![0xC000], "STM with empty list"),
        (
            vec![0x4500],
            "CMP of two low registers in the high-register form",
        ),
    ] {
        let mut cpu = cpu_with(&code);
        let mut stop = None;
        for _ in 0..code.len() {
            if let Err(s) = cpu.step(&mut NoBus) {
                stop = Some(s);
                break;
            }
        }
        assert!(
            matches!(stop, Some(Stop::Undefined { .. })),
            "{what}: {stop:?}"
        );
    }
}

#[test]
fn breakpoint_and_svc_stop_with_their_immediate() {
    let mut cpu = cpu_with(&[0xBEAB]);
    assert_eq!(
        cpu.step(&mut NoBus),
        Err(Stop::Breakpoint {
            pc: CODE,
            imm: 0xAB
        })
    );
    let mut cpu = cpu_with(&[0xDF05]);
    assert_eq!(cpu.step(&mut NoBus), Err(Stop::Svc { pc: CODE, imm: 5 }));
}

#[test]
fn interworking_branch_to_even_address_is_invalid_state() {
    // movs r0,#8 ; bx r0
    let mut cpu = cpu_with(&[0x2008, 0x4700]);
    run(&mut cpu, 1);
    assert_eq!(
        cpu.step(&mut NoBus),
        Err(Stop::InvalidState {
            pc: CODE + 2,
            target: 8
        })
    );
    assert_eq!(cpu.r[15], CODE + 2, "PC unchanged after the stop");
}

#[test]
fn exc_return_value_in_handler_mode_is_reported() {
    // mvns r0, r0 (r0 = 0xFFFFFFFF) ; subs r0,#6 (0xFFFFFFF9) ; bx r0
    let mut cpu = cpu_with(&[0x43C0, 0x3806, 0x4700]);
    cpu.ipsr = 15;
    run(&mut cpu, 2);
    assert_eq!(
        cpu.step(&mut NoBus),
        Err(Stop::ExceptionReturn {
            pc: CODE + 4,
            value: 0xFFFF_FFF9
        })
    );
}

#[test]
fn faulting_access_leaves_state_unchanged() {
    // movs r0,#1 ; lsls r0,#28 (0x10000000, unmapped) ; ldr r1,[r0] ; then
    // pop {r0,r1} from an unmapped SP
    let mut cpu = cpu_with(&[0x2001, 0x0700, 0x6801]);
    run(&mut cpu, 2);
    let before = cpu.r;
    let flags = cpu.flags;
    let stop = cpu.step(&mut NoBus);
    assert!(matches!(stop, Err(Stop::BusFault { .. })), "{stop:?}");
    assert_eq!(cpu.r, before);
    assert_eq!(cpu.flags, flags);
}

#[test]
fn pop_into_pc_is_rolled_back_on_a_bad_target() {
    // push {r0} with r0 = 0x200 (bit 0 clear) ; pop {pc}
    let mut cpu = cpu_with(&[0x2080, 0x0040, 0xB401, 0xBD00]);
    run(&mut cpu, 3); // movs r0,#0x80 ; lsls r0,r0,#1 ; push {r0}
    let sp = cpu.r[13];
    assert!(matches!(
        cpu.step(&mut NoBus),
        Err(Stop::InvalidState { .. })
    ));
    assert_eq!(cpu.r[13], sp, "SP restored");
}

#[test]
fn elf_loader_copies_load_segments_and_finds_entry() {
    // Hand-built ELF32 LE ARM: header (52 bytes), one PT_LOAD program
    // header (32 bytes) loading 4 bytes from offset 84 to 0x2000_0010.
    let mut f = vec![0u8; 88];
    f[0..4].copy_from_slice(b"\x7fELF");
    f[4] = 1; // 32-bit
    f[5] = 1; // little-endian
    f[18..20].copy_from_slice(&40u16.to_le_bytes()); // EM_ARM
    f[24..28].copy_from_slice(&0x101u32.to_le_bytes()); // entry
    f[28..32].copy_from_slice(&52u32.to_le_bytes()); // phoff
    f[42..44].copy_from_slice(&32u16.to_le_bytes()); // phentsize
    f[44..46].copy_from_slice(&1u16.to_le_bytes()); // phnum
    let ph = 52;
    f[ph..ph + 4].copy_from_slice(&1u32.to_le_bytes()); // PT_LOAD
    f[ph + 4..ph + 8].copy_from_slice(&84u32.to_le_bytes()); // offset
    f[ph + 12..ph + 16].copy_from_slice(&0x2000_0010u32.to_le_bytes()); // paddr
    f[ph + 16..ph + 20].copy_from_slice(&4u32.to_le_bytes()); // filesz
    f[84..88].copy_from_slice(&0xDEAD_BEEFu32.to_le_bytes());
    let elf = Elf::parse(f).unwrap();
    assert_eq!(elf.entry, 0x101);
    let mut m = Memory::new();
    m.add_region(0x2000_0000, 0x100, true);
    elf.load_into(&mut m).unwrap();
    assert_eq!(m.read(0x2000_0010, 4), Some(0xDEAD_BEEF));
    assert!(Elf::parse(b"not an elf".to_vec()).is_err());
}
