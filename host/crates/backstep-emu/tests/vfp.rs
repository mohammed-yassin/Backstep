//! Unit tests for the floating-point unit's instructions, for what the
//! QEMU differential test (`scripts/emu-difftest.py --fpu`) cannot show
//! on its own: where execution stops (NOCP, UNDEFINED, alignment) and
//! that a stopped instruction changed nothing, privileged-only access,
//! and the floating-point context (CONTROL.FPCA, FPDSCR).
//!
//! The encodings were produced with `arm-none-eabi-as -mfpu=fpv4-sp-d16`
//! and checked with `arm-none-eabi-objdump`; the comment on each one is
//! objdump's text.

use backstep_emu::cpu::vfp::fpccr;
use backstep_emu::memory::NoBus;
use backstep_emu::{Cpu, Memory, Stop};

const CODE: u32 = 0x100;
const RAM: u32 = 0x2000_0000;
const FULL_ACCESS: u32 = 0x00F0_0000;

#[allow(clippy::cast_possible_truncation)]
fn halfwords(insns: &[u32]) -> Vec<u16> {
    insns
        .iter()
        .flat_map(|i| [(i >> 16) as u16, (i & 0xFFFF) as u16])
        .collect()
}

/// A core at CODE running `insns`, the FPU enabled, ASPEN off (no
/// floating-point context bookkeeping unless a test turns it on).
fn cpu_with(insns: &[u32]) -> Cpu {
    let mut m = Memory::new();
    m.add_region(0, 0x1000, true);
    m.add_region(RAM, 0x1000, true);
    let bytes: Vec<u8> = halfwords(insns)
        .iter()
        .flat_map(|h| h.to_le_bytes())
        .collect();
    m.load(CODE, &bytes).unwrap();
    m.load(0, &(RAM + 0x800).to_le_bytes()).unwrap();
    m.load(4, &(CODE | 1).to_le_bytes()).unwrap();
    let mut cpu = Cpu::new(m);
    cpu.reset(0).unwrap();
    cpu.exceptions = false; // these tests check where execution stops
    cpu.scs.cpacr = FULL_ACCESS;
    cpu.scs.fpccr = 0;
    cpu
}

fn f(x: f32) -> u32 {
    x.to_bits()
}

fn run(cpu: &mut Cpu, n: usize) {
    for _ in 0..n {
        cpu.step(&mut NoBus).unwrap();
    }
}

const VADD: u32 = 0xEE30_0A81; // vadd.f32 s0, s1, s2

#[test]
fn coprocessor_access_is_checked_first() {
    // Disabled FPU: NOCP, with nothing changed.
    let mut cpu = cpu_with(&[VADD]);
    cpu.scs.cpacr = 0;
    cpu.s[1] = f(1.0);
    assert_eq!(cpu.step(&mut NoBus), Err(Stop::NoCoprocessor { pc: CODE }));
    assert_eq!((cpu.r[15], cpu.s[0]), (CODE, 0));
    // NOCP comes before decoding: an UNDEFINED double-precision encoding
    // with the FPU disabled is NOCP too (as QEMU's m-nocp.decode).
    let mut cpu = cpu_with(&[0xEE30_0B00]); // vadd.f64 d0, d0, d0
    cpu.scs.cpacr = 0;
    assert_eq!(cpu.step(&mut NoBus), Err(Stop::NoCoprocessor { pc: CODE }));
    cpu.scs.cpacr = FULL_ACCESS;
    assert_eq!(
        cpu.step(&mut NoBus),
        Err(Stop::Undefined {
            pc: CODE,
            insn: 0xEE30_0B00
        })
    );
    // Any other coprocessor is absent.
    let mut cpu = cpu_with(&[0xEE00_0010]); // mcr 0, 0, r0, cr0, cr0, {0}
    assert_eq!(cpu.step(&mut NoBus), Err(Stop::NoCoprocessor { pc: CODE }));
    // Privileged-only access (cp10 = 0b01).
    for (control, allowed) in [(0, true), (1, false)] {
        let mut cpu = cpu_with(&[VADD]);
        cpu.scs.cpacr = 0x0050_0000;
        cpu.control = control;
        let result = cpu.step(&mut NoBus);
        assert_eq!(result.is_ok(), allowed, "nPRIV = {control}: {result:?}");
    }
    // The reserved value 0b10 denies access (QEMU's choice).
    let mut cpu = cpu_with(&[VADD]);
    cpu.scs.cpacr = 0x00A0_0000;
    assert_eq!(cpu.step(&mut NoBus), Err(Stop::NoCoprocessor { pc: CODE }));
}

#[test]
fn armv8_and_simd_encodings_are_undefined() {
    for insn in [
        0xFE00_0A00u32, // vseleq.f32 s0, s0, s0 (ARMv8)
        0xEF00_0D40,    // vadd.f32 q0, q0, q0 (Advanced SIMD)
        0xEEB6_0A40,    // vrintr.f32 s0, s0 (ARMv8)
        0xEE80_0AC1,    // VDIV with op = 1: not allocated
        0xEEF0_0A10,    // VMRS from FPSID: only FPSCR on ARMv7-M
    ] {
        let mut cpu = cpu_with(&[insn]);
        assert_eq!(
            cpu.step(&mut NoBus),
            Err(Stop::Undefined { pc: CODE, insn }),
            "{insn:08x}"
        );
    }
}

#[test]
fn arithmetic_accumulates_flags_in_fpscr() {
    let mut cpu = cpu_with(&[
        VADD,
        0xEE80_0A81, // vdiv.f32 s0, s1, s2
        0xEEF1_0A10, // vmrs r0, fpscr
        0xEEF1_FA10, // vmrs APSR_nzcv, fpscr
    ]);
    cpu.s[1] = f(1.0);
    cpu.s[2] = f(3.0);
    run(&mut cpu, 2);
    assert_eq!(cpu.s[0], f(1.0 / 3.0));
    assert_eq!(cpu.fpscr, 0x10, "IXC");
    cpu.fpscr |= 0xA000_0000; // N, C
    cpu.flags.q = true;
    run(&mut cpu, 2);
    assert_eq!(cpu.r[0], 0xA000_0010);
    assert!(cpu.flags.n && !cpu.flags.z && cpu.flags.c && !cpu.flags.v);
    assert!(cpu.flags.q, "APSR.Q is not written");
}

#[test]
fn vmsr_writes_only_implemented_bits() {
    let mut cpu = cpu_with(&[0xEEE1_1A10]); // vmsr fpscr, r1
    cpu.r[1] = u32::MAX;
    run(&mut cpu, 1);
    assert_eq!(cpu.fpscr, 0xF7C0_009F);
}

#[test]
fn a_new_floating_point_context_takes_fpdscr() {
    // ASPEN set, FPCA clear: the first FP instruction sets FPCA and
    // starts from FPDSCR (round towards zero here); the second keeps it.
    let mut cpu = cpu_with(&[VADD, 0xEEE1_1A10, VADD]); // vadd; vmsr fpscr, r1; vadd
    cpu.scs.fpccr = fpccr::ASPEN;
    cpu.scs.fpdscr = 3 << 22;
    cpu.fpscr = 0x1F;
    cpu.s[1] = f(1.0);
    cpu.s[2] = f(1e-10);
    run(&mut cpu, 1);
    assert_eq!(cpu.control & 4, 4, "FPCA");
    assert_eq!(cpu.s[0], f(1.0), "rounded towards zero");
    assert_eq!(cpu.fpscr, (3 << 22) | 0x10, "FPSCR from FPDSCR, then IXC");
    cpu.r[1] = 0; // round to nearest
    run(&mut cpu, 2);
    assert_eq!(cpu.fpscr, 0x10, "same context: FPSCR kept");
    // Without ASPEN nothing changes.
    let mut cpu = cpu_with(&[VADD]);
    cpu.scs.fpdscr = 3 << 22;
    run(&mut cpu, 1);
    assert_eq!((cpu.control, cpu.fpscr), (0, 0));
}

#[test]
fn lazy_preservation_writes_the_reserved_space_first() {
    // LSPACT set: the first FP instruction stores S0-S15 and FPSCR (the
    // interrupted context's) at FPCAR, then runs on the same registers.
    let mut cpu = cpu_with(&[VADD]);
    cpu.scs.fpccr = fpccr::LSPACT;
    cpu.scs.fpcar = RAM + 0x200;
    for (s, v) in cpu.s.iter_mut().zip(0x100..) {
        *s = v;
    }
    cpu.fpscr = 0x10;
    run(&mut cpu, 1);
    assert_eq!(cpu.scs.fpccr & fpccr::LSPACT, 0);
    for i in 0..16 {
        assert_eq!(cpu.memory.read(RAM + 0x200 + 4 * i, 4), Some(0x100 + i));
    }
    assert_eq!(
        cpu.memory.read(RAM + 0x240, 4),
        Some(0x10),
        "FPSCR at +0x40"
    );
    assert_eq!(
        cpu.memory.read(RAM + 0x244, 4),
        Some(0),
        "S16 is not stored"
    );
    // An unprivileged background context without access (CPACR
    // privileged-only, FPCCR.USER): not emulated (a NOCP UsageFault).
    let mut cpu = cpu_with(&[VADD]);
    cpu.scs.cpacr = 0x0050_0000;
    cpu.scs.fpccr = fpccr::LSPACT | fpccr::USER;
    assert!(matches!(
        cpu.step(&mut NoBus),
        Err(Stop::Unsupported { .. })
    ));
}

#[test]
fn the_floating_point_check_stays_when_the_instruction_faults() {
    // As in QEMU: a VLDR that faults has still created the context.
    let mut cpu = cpu_with(&[0xEDD7_1A02]); // vldr s3, [r7, #8]
    cpu.scs.fpccr = fpccr::ASPEN;
    cpu.scs.fpdscr = 1 << 24;
    cpu.r[7] = RAM + 2;
    assert!(matches!(cpu.step(&mut NoBus), Err(Stop::Unaligned { .. })));
    assert_eq!((cpu.control & 4, cpu.fpscr), (4, 1 << 24));
}

#[test]
fn loads_and_stores_move_words_in_register_order() {
    let mut cpu = cpu_with(&[
        0xECA5_0A20, // vstmia r5!, {s0-s31}
        0xED35_0B20, // vldmdb r5!, {d0-d15}
        0xED2D_2B06, // vpush {d2-d4}
        0xECBD_1A08, // vpop {s2-s9}
        0xEDD7_1A02, // vldr s3, [r7, #8]
        0xED17_3B02, // vldr d3, [r7, #-8]
        0xED87_3BFF, // vstr d3, [r7, #1020]
    ]);
    for (s, v) in cpu.s.iter_mut().zip(0x100..) {
        *s = v;
    }
    cpu.r[5] = RAM + 0x100;
    cpu.r[7] = RAM + 0x400;
    run(&mut cpu, 1);
    assert_eq!(cpu.r[5], RAM + 0x180);
    assert_eq!(cpu.memory.read(RAM + 0x100, 4), Some(0x100));
    assert_eq!(cpu.memory.read(RAM + 0x17C, 4), Some(0x11F));
    cpu.s = [0; 32];
    run(&mut cpu, 1);
    assert_eq!(cpu.r[5], RAM + 0x100);
    assert_eq!(cpu.s[0], 0x100);
    assert_eq!(cpu.s[31], 0x11F);
    let sp = cpu.r[13];
    run(&mut cpu, 1);
    assert_eq!(cpu.r[13], sp - 24);
    assert_eq!(cpu.memory.read(sp - 24, 4), Some(0x104), "s4 = d2 low");
    cpu.s[2..10].copy_from_slice(&[0; 8]);
    run(&mut cpu, 1);
    assert_eq!(cpu.r[13], sp + 8, "8 words popped, 6 pushed");
    assert_eq!(
        &cpu.s[2..10],
        &[0x104, 0x105, 0x106, 0x107, 0x108, 0x109, 0, 0]
    );
    cpu.memory.write(RAM + 0x408, 4, 0xAAAA_AAAA).unwrap();
    cpu.memory.write(RAM + 0x3F8, 4, 0x1111_1111).unwrap();
    cpu.memory.write(RAM + 0x3FC, 4, 0x2222_2222).unwrap();
    run(&mut cpu, 3);
    assert_eq!(cpu.s[3], 0xAAAA_AAAA);
    assert_eq!((cpu.s[6], cpu.s[7]), (0x1111_1111, 0x2222_2222));
    assert_eq!(cpu.memory.read(RAM + 0x7FC, 4), Some(0x1111_1111));
    assert_eq!(cpu.memory.read(RAM + 0x800, 4), Some(0x2222_2222));
}

#[test]
fn stopped_loads_change_nothing() {
    // Unaligned base: UNALIGNED, whatever CCR says (MemA).
    let mut cpu = cpu_with(&[0xEDD7_1A02]); // vldr s3, [r7, #8]
    cpu.r[7] = RAM + 2;
    assert_eq!(
        cpu.step(&mut NoBus),
        Err(Stop::Unaligned {
            pc: CODE,
            address: RAM + 10
        })
    );
    // A bus fault on the last word of a VLDM: no register written, no
    // writeback.
    let mut cpu = cpu_with(&[0xECB5_2B06]); // vldmia r5!, {d2-d4}
    cpu.r[5] = RAM + 0x1000 - 20;
    let before = cpu.s;
    assert!(matches!(cpu.step(&mut NoBus), Err(Stop::BusFault { .. })));
    assert_eq!((cpu.s, cpu.r[5]), (before, RAM + 0x1000 - 20));
}

#[test]
fn register_transfers() {
    let mut cpu = cpu_with(&[
        0xEE00_0A90, // vmov s1, r0
        0xEC41_0A12, // vmov s4, s5, r0, r1
        0xEC41_0B13, // vmov d3, r0, r1
        0xEE22_3B10, // vmov.32 d2[1], r3
        0xEE10_0A90, // vmov r0, s1
        0xEC51_0B13, // vmov r0, r1, d3
        0xEE12_3B10, // vmov.32 r3, d2[0]
    ]);
    cpu.r[0] = 10;
    cpu.r[1] = 11;
    cpu.r[3] = 33;
    run(&mut cpu, 4);
    assert_eq!(cpu.s[1], 10);
    assert_eq!((cpu.s[4], cpu.s[5]), (10, 33), "s5 = d2[1] = r3");
    assert_eq!((cpu.s[6], cpu.s[7]), (10, 11));
    cpu.r = [0; 16];
    cpu.r[15] = CODE + 16;
    run(&mut cpu, 3);
    assert_eq!((cpu.r[0], cpu.r[1], cpu.r[3]), (10, 11, 10));
}

#[test]
fn compare_and_conversions() {
    let mut cpu = cpu_with(&[
        0xEEB4_0A60, // vcmp.f32 s0, s1
        0xEEB5_0AC0, // vcmpe.f32 s0, #0.0
        0xEEBE_0A66, // vcvt.s16.f32 s0, s0, #3
        0xEEB3_0AE0, // vcvtt.f16.f32 s0, s1
        0xEEBC_0A01, // vmov.f32 s0, #193 (-0.1328125)
    ]);
    cpu.s[0] = f(-2.5);
    cpu.s[1] = f(1.0);
    run(&mut cpu, 1);
    assert_eq!(cpu.fpscr, 0x8000_0000, "less than: N");
    cpu.s[0] = 0x7FC0_0000; // quiet NaN: VCMPE raises IOC
    run(&mut cpu, 1);
    assert_eq!(cpu.fpscr, 0x3000_0001, "unordered: C, V; IOC");
    cpu.s[0] = f(-2.5);
    run(&mut cpu, 1);
    assert_eq!(cpu.s[0], 0xFFFF_FFEC, "-2.5 * 8 = -20, sign-extended");
    run(&mut cpu, 1);
    assert_eq!(cpu.s[0], 0x3C00_FFEC, "1.0 in the top half, bottom kept");
    run(&mut cpu, 1);
    assert_eq!(cpu.s[0], f(-0.132_812_5));
}

#[test]
fn floating_point_instructions_run_in_it_blocks() {
    let mut cpu = cpu_with(&[
        0xBF04_EE30, // itt eq; first half of vaddeq.f32 s0, s1, s2
        0x0A81_EE00, // second half; vmlseq.f32 s0, s1, s2 (first half)
        0x0AC1_0000, // second half; padding
    ]);
    cpu.s[1] = f(2.0);
    cpu.s[2] = f(3.0);
    cpu.flags.z = false;
    run(&mut cpu, 3);
    assert_eq!(cpu.s[0], 0, "both skipped");
    assert_eq!(cpu.r[15], CODE + 10);
}

/// A core that takes exceptions: thread code at CODE (`svc #0; nop`),
/// the `SVCall` handler at 0x200 running `handler`, then `bx lr`.
fn cpu_with_svc_handler(handler: &[u32]) -> Cpu {
    let mut cpu = cpu_with(&[0xDF00_BF00]); // svc #0; nop
    let mut code = halfwords(handler);
    code.push(0x4770); // bx lr
    let bytes: Vec<u8> = code.iter().flat_map(|h| h.to_le_bytes()).collect();
    cpu.memory.load(0x200, &bytes).unwrap();
    cpu.memory.load(4 * 11, &0x201u32.to_le_bytes()).unwrap();
    cpu.exceptions = true;
    cpu.scs.fpccr = fpccr::ASPEN | fpccr::LSPEN;
    cpu.control = 4; // a floating-point context is active
    for (s, v) in cpu.s.iter_mut().zip(0x100..) {
        *s = v;
    }
    cpu.fpscr = 0x0300_0010; // DN, FZ... and IXC
    cpu
}

#[test]
fn an_exception_with_a_floating_point_context_reserves_an_extended_frame() {
    let mut cpu = cpu_with_svc_handler(&[VADD]);
    let sp = cpu.r[13];
    run(&mut cpu, 1); // svc: entry
    let frame = sp - 0x68;
    assert_eq!(cpu.r[13], frame, "26 words");
    assert_eq!(cpu.r[14], 0xFFFF_FFE9, "EXC_RETURN with FType clear");
    assert_eq!(cpu.control & 4, 0, "FPCA cleared in the handler");
    assert_eq!(cpu.scs.fpcar, frame + 0x20);
    assert_ne!(
        cpu.scs.fpccr & fpccr::LSPACT,
        0,
        "lazy: nothing written yet"
    );
    assert_eq!(
        cpu.scs.fpccr & (fpccr::USER | fpccr::THREAD | fpccr::HFRDY),
        fpccr::THREAD | fpccr::HFRDY
    );
    assert_eq!(cpu.memory.read(frame + 0x20, 4), Some(0), "S0 not stored");
    // The handler's first FP instruction preserves the thread's state,
    // and starts a new context from FPDSCR.
    run(&mut cpu, 1);
    assert_eq!(cpu.scs.fpccr & fpccr::LSPACT, 0);
    assert_eq!(cpu.memory.read(frame + 0x20, 4), Some(0x100), "S0");
    assert_eq!(cpu.memory.read(frame + 0x5C, 4), Some(0x10F), "S15");
    assert_eq!(cpu.memory.read(frame + 0x60, 4), Some(0x0300_0010), "FPSCR");
    assert_eq!(cpu.control & 4, 4);
    assert_ne!(cpu.s[0], 0x100, "the handler changed S0");
    // Return: S0-S15 and FPSCR come back from the frame, FPCA is set.
    run(&mut cpu, 1);
    assert_eq!(cpu.r[13], sp);
    assert_eq!(cpu.ipsr, 0);
    assert_eq!(
        (cpu.s[0], cpu.fpscr, cpu.control & 4),
        (0x100, 0x0300_0010, 4)
    );
}

#[test]
fn without_lazy_stacking_the_registers_are_stored_at_once() {
    let mut cpu = cpu_with_svc_handler(&[]);
    cpu.scs.fpccr = fpccr::ASPEN;
    let sp = cpu.r[13];
    run(&mut cpu, 1);
    let frame = sp - 0x68;
    assert_eq!(cpu.memory.read(frame + 0x24, 4), Some(0x101), "S1");
    assert_eq!(cpu.memory.read(frame + 0x60, 4), Some(0x0300_0010));
    assert_eq!(cpu.scs.fpccr & fpccr::LSPACT, 0);
    // QEMU clears them (the architecture: UNKNOWN); S16-S31 are kept.
    assert_eq!((cpu.s[1], cpu.fpscr, cpu.s[16]), (0, 0, 0x110));
    run(&mut cpu, 1); // bx lr
    assert_eq!((cpu.s[1], cpu.fpscr), (0x101, 0x0300_0010));
}

#[test]
fn a_handler_without_floating_point_returns_lazily() {
    // LSPACT is still set at the return: the registers were never
    // touched, so only LSPACT is cleared and the frame is not read.
    let mut cpu = cpu_with_svc_handler(&[]);
    let sp = cpu.r[13];
    run(&mut cpu, 1);
    cpu.memory.write(sp - 0x68 + 0x20, 4, 0xDEAD).unwrap(); // S0's slot
    run(&mut cpu, 1);
    assert_eq!(cpu.scs.fpccr & fpccr::LSPACT, 0);
    assert_eq!((cpu.s[0], cpu.r[13], cpu.control & 4), (0x100, sp, 4));
}
