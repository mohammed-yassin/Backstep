//! Unit tests for 32-bit Thumb-2 behaviour the QEMU differential test
//! cannot reach: UNPREDICTABLE encodings, alignment faults, branch offsets
//! larger than the 4 MiB test image, privilege, stack-pointer banking, the
//! exclusive monitor's failure cases, and rollback after a fault.
//!
//! The encodings were produced (and checked) with `arm-none-eabi-as` and
//! `arm-none-eabi-objdump`; the comment on each one is objdump's text.

use backstep_emu::memory::NoBus;
use backstep_emu::{Cpu, Memory, Stop};

const CODE: u32 = 0x100;
const RAM: u32 = 0x2000_0000;

/// Halfwords of a 32-bit instruction, first halfword first.
#[allow(clippy::cast_possible_truncation)]
fn w(insn: u32) -> [u16; 2] {
    [(insn >> 16) as u16, (insn & 0xFFFF) as u16]
}

fn cpu_at(base: u32, code: &[u16]) -> Cpu {
    let mut m = Memory::new();
    m.add_region(0, 0x1000, true);
    if base >= 0x1000 {
        m.add_region(base, 0x100, true);
    }
    m.add_region(RAM, 0x1000, true);
    let bytes: Vec<u8> = code.iter().flat_map(|h| h.to_le_bytes()).collect();
    m.load(base, &bytes).unwrap();
    m.load(0, &(RAM + 0x800).to_le_bytes()).unwrap();
    m.load(4, &(base | 1).to_le_bytes()).unwrap();
    let mut cpu = Cpu::new(m);
    cpu.reset(0).unwrap();
    cpu
}

fn cpu_with(code: &[u16]) -> Cpu {
    cpu_at(CODE, code)
}

fn step(cpu: &mut Cpu) {
    cpu.step(&mut NoBus).unwrap();
}

#[test]
fn far_branches_decode_every_offset_bit() {
    // (address, encoding, target, links)
    for (pc, insn, target, link) in [
        (0x0100_0000, 0xF3FF_DFFE, 0x01C0_0000, true), // bl 1c00000
        (0x0100_0004, 0xF400_987C, 0x0040_0100, false), // b.w 400100
        (0x0100_0008, 0xF400_D002, 0x0000_0010, true), // bl 10
        (0x0100_000C, 0xF400_8000, 0x00F0_0010, false), // beq.w f00010 (Z set below)
        (0x0100_0010, 0xF2BC_9EE6, 0x01AB_CDE0, false), // b.w 1abcde0
    ] {
        let mut cpu = cpu_at(pc, &w(insn));
        cpu.flags.z = true;
        step(&mut cpu);
        assert_eq!(cpu.r[15], target, "{insn:08x}");
        if link {
            assert_eq!(cpu.r[14], (pc + 4) | 1, "{insn:08x} sets LR");
        }
    }
}

#[test]
fn unpredictable_and_undefined_32bit_encodings_stop() {
    for (code, what) in [
        (
            w(0xE890_0002).to_vec(),
            "ldmia.w r0, {r1}: fewer than two registers",
        ),
        (
            w(0xE9D0_1100).to_vec(),
            "ldrd r1, r1, [r0]: same register twice",
        ),
        (w(0xF361_2004).to_vec(), "bfi with msb below lsb"),
        (w(0xF240_0D00).to_vec(), "movw sp, #0"),
        (
            w(0xE840_1100).to_vec(),
            "strex r1, r1, [r0]: status = data register",
        ),
        (w(0xF7F0_A000).to_vec(), "udf.w #0"),
        (w(0xF000_C000).to_vec(), "blx (immediate): not in ARMv7-M"),
        (
            [&[0xBF04u16][..], &w(0xF8D0_F000), &[0xBF00]].concat(),
            "itt eq; ldr.w pc, [r0] not last in the IT block",
        ),
        (
            [&[0xBF08u16][..], &w(0xF000_8000)].concat(),
            "it eq; beq.w (T3): conditional branch inside IT",
        ),
    ] {
        let mut cpu = cpu_with(&code);
        cpu.r[0] = RAM;
        let mut stop = None;
        for _ in 0..3 {
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
fn floating_point_is_reported_not_guessed() {
    let mut cpu = cpu_with(&w(0xEE30_0A00)); // vadd.f32 s0, s0, s0
    assert_eq!(
        cpu.step(&mut NoBus),
        Err(Stop::Unimplemented {
            pc: CODE,
            insn: 0xEE30_0A00
        })
    );
}

#[test]
fn alignment_is_required_where_the_architecture_requires_it() {
    for (insn, what) in [
        (0xE9D2_0100u32, "ldrd r0, r1, [r2]"),
        (0xE892_0003, "ldmia.w r2, {r0, r1}"),
        (0xE852_1F00, "ldrex r1, [r2]"),
    ] {
        let mut cpu = cpu_with(&w(insn));
        cpu.r[2] = RAM + 2;
        assert_eq!(
            cpu.step(&mut NoBus),
            Err(Stop::Unaligned {
                pc: CODE,
                address: RAM + 2
            }),
            "{what}"
        );
    }
    // Plain LDR.W may be unaligned (CCR.UNALIGN_TRP = 0).
    let mut cpu = cpu_with(&w(0xF8D2_1000)); // ldr.w r1, [r2]
    cpu.memory.load(RAM, &[1, 2, 3, 4, 5, 6]).unwrap();
    cpu.r[2] = RAM + 1;
    step(&mut cpu);
    assert_eq!(cpu.r[1], 0x0504_0302);
    // 16-bit PUSH with an unaligned SP.
    let mut cpu = cpu_with(&[0xB401]); // push {r0}
    cpu.r[13] = RAM + 0x802;
    assert!(matches!(cpu.step(&mut NoBus), Err(Stop::Unaligned { .. })));
}

#[test]
fn exclusive_monitor_fails_after_clrex_and_on_another_address() {
    // ldrex r1, [r0] ; clrex ; strex r2, r1, [r0]
    let code = [w(0xE850_1F00), w(0xF3BF_8F2F), w(0xE840_1200)].concat();
    let mut cpu = cpu_with(&code);
    cpu.r[0] = RAM;
    for _ in 0..3 {
        step(&mut cpu);
    }
    assert_eq!(cpu.r[2], 1, "STREX after CLREX fails");
    // ldrex r1, [r0] ; strex r2, r1, [r0, #4]
    let mut cpu = cpu_with(&[w(0xE850_1F00), w(0xE840_1201)].concat());
    cpu.r[0] = RAM;
    step(&mut cpu);
    step(&mut cpu);
    assert_eq!(cpu.r[2], 1, "STREX to another address fails");
    assert_eq!(cpu.monitor, None, "and clears the monitor");
    // ...unless the monitor does not tag addresses.
    let mut cpu = cpu_with(&[w(0xE850_1F00), w(0xE840_1201)].concat());
    cpu.exclusive_tags_address = false;
    cpu.r[0] = RAM;
    step(&mut cpu);
    step(&mut cpu);
    assert_eq!(cpu.r[2], 0, "untagged monitor: any address passes");
    // ldrex r1, [r0] ; strex r2, r3, [r0]: succeeds and writes.
    let mut cpu = cpu_with(&[w(0xE850_1F00), w(0xE840_3200)].concat());
    cpu.r[0] = RAM;
    cpu.r[3] = 0xCAFE_F00D;
    step(&mut cpu);
    step(&mut cpu);
    assert_eq!(cpu.r[2], 0);
    assert_eq!(cpu.memory.read(RAM, 4), Some(0xCAFE_F00D));
}

#[test]
fn unprivileged_writes_to_special_registers_are_ignored() {
    // msr primask, r0 ; msr basepri, r0 ; cpsid i ; mrs r3, msp
    let code = [
        &w(0xF380_8810)[..],
        &w(0xF380_8811),
        &[0xB672],
        &w(0xF3EF_8308),
    ]
    .concat();
    let mut cpu = cpu_with(&code);
    cpu.control = 1; // nPRIV
    cpu.r[0] = 0xFF;
    cpu.r[3] = 0x1234;
    for _ in 0..4 {
        step(&mut cpu);
    }
    assert!(!cpu.primask);
    assert_eq!(cpu.basepri, 0);
    assert_eq!(cpu.r[3], 0, "MSP reads as zero when unprivileged");
}

#[test]
fn control_spsel_banks_the_stack_pointer_in_thread_mode_only() {
    // msr psp, r0 ; msr control, r1 ; mrs r3, psp
    let code = [w(0xF380_8809), w(0xF381_8814), w(0xF3EF_8309)].concat();
    let mut cpu = cpu_with(&code);
    let msp = cpu.r[13];
    cpu.r[0] = RAM + 0x400;
    cpu.r[1] = 2; // SPSEL
    for _ in 0..3 {
        step(&mut cpu);
    }
    assert_eq!(cpu.r[13], RAM + 0x400, "thread mode now runs on PSP");
    assert_eq!((cpu.msp(), cpu.psp()), (msp, RAM + 0x400));
    assert_eq!(cpu.r[3], RAM + 0x400);
    // In handler mode SPSEL is not writable.
    let mut cpu = cpu_with(&w(0xF381_8814));
    cpu.ipsr = 15;
    cpu.r[1] = 2;
    step(&mut cpu);
    assert_eq!(cpu.control & 2, 0);
}

#[test]
fn faultmask_cannot_be_set_in_hardfault() {
    // msr faultmask, r0 ; cpsid f
    let mut cpu = cpu_with(&[&w(0xF380_8813)[..], &[0xB671]].concat());
    cpu.ipsr = 3;
    cpu.r[0] = 1;
    step(&mut cpu);
    step(&mut cpu);
    assert!(!cpu.faultmask);
    // In thread mode both set it, and cpsie f clears it.
    let mut cpu = cpu_with(&[&w(0xF380_8813)[..], &[0xB661, 0xB671]].concat());
    cpu.r[0] = 1;
    step(&mut cpu);
    assert!(cpu.faultmask);
    step(&mut cpu);
    assert!(!cpu.faultmask);
    step(&mut cpu);
    assert!(cpu.faultmask);
}

#[test]
fn division_by_zero_gives_zero_and_overflow_wraps() {
    // udiv r3, r1, r2 ; sdiv r3, r1, r2
    let mut cpu = cpu_with(&[w(0xFBB1_F3F2), w(0xFB91_F3F2)].concat());
    cpu.r[1] = 1234;
    cpu.r[2] = 0;
    step(&mut cpu);
    assert_eq!(cpu.r[3], 0);
    cpu.r[1] = 0x8000_0000;
    cpu.r[2] = 0xFFFF_FFFF;
    step(&mut cpu);
    assert_eq!(cpu.r[3], 0x8000_0000, "INT_MIN / -1");
}

#[test]
fn faulting_ldm_leaves_registers_unchanged() {
    // ldmia.w r0, {r1, r2} with the second word outside memory
    let mut cpu = cpu_with(&w(0xE890_0006));
    cpu.r[0] = RAM + 0xFFC;
    cpu.r[1] = 0x1111;
    let before = cpu.r;
    assert!(matches!(cpu.step(&mut NoBus), Err(Stop::BusFault { .. })));
    assert_eq!(cpu.r, before);
}

#[test]
fn load_into_pc_needs_the_thumb_bit() {
    // ldr.w pc, [r0] loading an even address
    let mut cpu = cpu_with(&w(0xF8D0_F000));
    cpu.r[0] = RAM;
    cpu.memory.write(RAM, 4, 0x200);
    assert_eq!(
        cpu.step(&mut NoBus),
        Err(Stop::InvalidState {
            pc: CODE,
            target: 0x200
        })
    );
}
