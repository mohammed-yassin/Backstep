//! Unit tests for exception behaviour the QEMU differential test does not
//! reach: instruction-fetch bus errors, unprivileged SCS accesses, the
//! replay API for raising interrupts, lockup, the monitor on exception
//! entry, and the cases the emulator reports as unsupported instead of
//! guessing.

use backstep_emu::cpu::exceptions::{cfsr, BUS_FAULT, HARD_FAULT, IRQ0, SVCALL, USAGE_FAULT};
use backstep_emu::memory::NoBus;
use backstep_emu::{Cpu, Memory, Stop};

const CODE: u32 = 0x200;
const HANDLER: u32 = 0x400; // every vector points here: bx lr
const RAM: u32 = 0x2000_0000;
const SP: u32 = RAM + 0x800;

fn cpu_with(code: &[u16]) -> Cpu {
    let mut m = Memory::new();
    m.add_region(0, 0x1000, true);
    m.add_region(RAM, 0x1000, true);
    let bytes: Vec<u8> = code.iter().flat_map(|h| h.to_le_bytes()).collect();
    m.load(CODE, &bytes).unwrap();
    m.load(HANDLER, &0x4770u16.to_le_bytes()).unwrap(); // bx lr
    m.load(0, &SP.to_le_bytes()).unwrap();
    m.load(4, &(CODE | 1).to_le_bytes()).unwrap();
    for n in 2..64u32 {
        m.load(4 * n, &(HANDLER | 1).to_le_bytes()).unwrap();
    }
    let mut cpu = Cpu::new(m);
    cpu.reset(0).unwrap();
    cpu
}

fn step(cpu: &mut Cpu) {
    cpu.step(&mut NoBus).unwrap();
}

fn stacked(cpu: &Cpu, word: u32) -> u32 {
    cpu.memory.read(cpu.r[13] + 4 * word, 4).unwrap()
}

#[test]
fn instruction_fetch_bus_error_is_ibuserr_with_the_target_stacked() {
    // ldr r0, =0x5001 (via movs/lsls) ; bx r0 -> fetch from 0x5000 fails
    let mut cpu = cpu_with(&[0x2005, 0x0300, 0x3001, 0x4700]); // movs r0,#5; lsls r0,#12; adds r0,#1; bx r0
    cpu.scs.shcsr_enable = 1 << 17; // BUSFAULTENA
    for _ in 0..4 {
        step(&mut cpu);
    }
    step(&mut cpu); // the fetch at 0x5000 faults: BusFault entered
    assert_eq!(cpu.ipsr as usize, BUS_FAULT);
    assert_eq!(cpu.scs.cfsr, cfsr::IBUSERR);
    assert_eq!(stacked(&cpu, 6), 0x5000, "stacked PC is the failed fetch");
    assert_eq!(cpu.r[15], HANDLER);
}

#[test]
fn disabled_fault_escalates_to_hardfault_with_forced() {
    let mut cpu = cpu_with(&[0xDE00]); // udf #0
    step(&mut cpu);
    assert_eq!(cpu.ipsr as usize, HARD_FAULT);
    assert_eq!(cpu.scs.hfsr, 1 << 30);
    assert_eq!(cpu.scs.cfsr, cfsr::UNDEFINSTR);
    assert_eq!(
        stacked(&cpu, 6),
        CODE,
        "stacked PC is the faulting instruction"
    );
    assert_eq!(cpu.stats.escalated, 1);
}

#[test]
fn enabled_usage_fault_is_taken_and_returns() {
    // udf #0 ; nop
    let mut cpu = cpu_with(&[0xDE00, 0xBF00]);
    cpu.scs.shcsr_enable = 1 << 18;
    step(&mut cpu);
    assert_eq!(cpu.ipsr as usize, USAGE_FAULT);
    assert_eq!(cpu.r[14], 0xFFFF_FFF9);
    step(&mut cpu); // bx lr: back to the udf (the handler did not skip it)
    assert_eq!((cpu.ipsr, cpu.r[15], cpu.r[13]), (0, CODE, SP));
}

#[test]
fn svc_stacks_the_next_instruction_and_escalates_when_masked() {
    let mut cpu = cpu_with(&[0xDF07]); // svc #7
    step(&mut cpu);
    assert_eq!(cpu.ipsr as usize, SVCALL);
    assert_eq!(stacked(&cpu, 6), CODE + 2);
    // With PRIMASK set SVC cannot be taken: HardFault, same return address.
    let mut cpu = cpu_with(&[0xDF07]);
    cpu.primask = true;
    step(&mut cpu);
    assert_eq!(cpu.ipsr as usize, HARD_FAULT);
    assert_eq!(stacked(&cpu, 6), CODE + 2);
    assert_eq!(cpu.scs.cfsr, 0, "an escalated SVC sets no CFSR bit");
}

#[test]
fn fault_in_hardfault_is_lockup() {
    let mut cpu = cpu_with(&[0xDE00]);
    cpu.memory.load(HANDLER, &0xDE01u16.to_le_bytes()).unwrap(); // the handler faults too
    step(&mut cpu); // HardFault entered
    assert_eq!(
        cpu.step(&mut NoBus),
        Err(Stop::Lockup { pc: HANDLER }),
        "a fault at priority -1 cannot be escalated"
    );
}

#[test]
fn unprivileged_scs_access_is_a_precise_bus_fault() {
    // ldr r1, [r0] with r0 = ICSR, from unprivileged thread mode
    let mut cpu = cpu_with(&[0x6801]);
    cpu.control = 1;
    cpu.r[0] = 0xE000_ED04;
    step(&mut cpu);
    assert_eq!(
        cpu.ipsr as usize, HARD_FAULT,
        "BusFault disabled: escalated"
    );
    assert_eq!(cpu.scs.cfsr, cfsr::PRECISERR | cfsr::BFARVALID);
    assert_eq!(cpu.scs.bfar, 0xE000_ED04);
}

#[test]
fn replay_can_raise_interrupts_and_priorities_decide() {
    let mut cpu = cpu_with(&[0xBF00, 0xBF00, 0xBF00]); // nops
    cpu.scs.enabled[IRQ0 + 3] = true;
    cpu.scs.enabled[IRQ0 + 4] = true;
    cpu.scs.priority[IRQ0 + 3] = 0x80;
    cpu.scs.priority[IRQ0 + 4] = 0x40;
    cpu.set_pending(IRQ0 + 3);
    cpu.set_pending(IRQ0 + 4);
    step(&mut cpu); // nop, then the more urgent IRQ4
    assert_eq!(cpu.ipsr as usize, IRQ0 + 4);
    assert_eq!(stacked(&cpu, 6), CODE + 2);
    step(&mut cpu); // bx lr: IRQ3 tail-chains, no unstacking
    assert_eq!(cpu.ipsr as usize, IRQ0 + 3);
    assert_eq!(cpu.stats.tail_chained, 1);
    assert_eq!(cpu.r[14], 0xFFFF_FFF9);
    step(&mut cpu);
    assert_eq!((cpu.ipsr, cpu.r[15], cpu.r[13]), (0, CODE + 2, SP));
    // A disabled interrupt stays pending.
    cpu.set_pending(IRQ0 + 5);
    step(&mut cpu);
    assert_eq!(cpu.ipsr, 0);
    assert!(cpu.scs.pending[IRQ0 + 5]);
}

#[test]
fn interrupt_now_enters_at_once_or_leaves_pending_as_it_was() {
    let mut cpu = cpu_with(&[0xBF00, 0xBF00, 0xBF00]); // nops
    cpu.scs.enabled[IRQ0 + 3] = true;
    cpu.scs.priority[IRQ0 + 3] = 0x80;
    step(&mut cpu);
    assert_eq!(cpu.interrupt_now(&mut NoBus, IRQ0 + 3), Ok(true));
    assert_eq!(
        cpu.ipsr as usize,
        IRQ0 + 3,
        "entered before the next instruction"
    );
    assert_eq!(stacked(&cpu, 6), CODE + 2);
    assert!(!cpu.scs.pending[IRQ0 + 3]);
    // Inside it, the same priority cannot preempt: nothing changes.
    let r = cpu.r;
    assert_eq!(cpu.interrupt_now(&mut NoBus, IRQ0 + 3), Ok(false));
    assert!(!cpu.scs.pending[IRQ0 + 3]);
    assert_eq!(cpu.r, r);
    // An interrupt that was already pending stays pending.
    cpu.scs.enabled[IRQ0 + 6] = true;
    cpu.scs.priority[IRQ0 + 6] = 0xC0;
    cpu.set_pending(IRQ0 + 6);
    assert_eq!(cpu.interrupt_now(&mut NoBus, IRQ0 + 6), Ok(false));
    assert!(cpu.scs.pending[IRQ0 + 6]);
    // PRIMASK masks it too.
    step(&mut cpu); // bx lr: IRQ6 tail-chains
    step(&mut cpu); // and returns
    cpu.primask = true;
    assert_eq!(cpu.interrupt_now(&mut NoBus, IRQ0 + 3), Ok(false));
    assert_eq!(cpu.ipsr, 0);
}

#[test]
fn exception_entry_clears_the_exclusive_monitor() {
    let mut cpu = cpu_with(&[0xBF00]);
    cpu.monitor = Some(RAM);
    cpu.set_pending(14); // PendSV
    step(&mut cpu);
    assert_eq!(cpu.monitor, None);
}

#[test]
fn unsupported_cases_stop_instead_of_guessing() {
    // An extended frame (CONTROL.FPCA) stored at once (no lazy stacking)
    // while CPACR forbids access to the FPU: a derived NOCP fault.
    let mut cpu = cpu_with(&[0xDF00]);
    cpu.control = 4;
    cpu.scs.fpccr = 0;
    assert!(matches!(
        cpu.step(&mut NoBus),
        Err(Stop::Unsupported { .. })
    ));
    // Stacking onto unmapped memory (a derived fault).
    let mut cpu = cpu_with(&[0xDF00]);
    cpu.r[13] = 0x1000_0000;
    assert!(matches!(
        cpu.step(&mut NoBus),
        Err(Stop::Unsupported { .. })
    ));
    // An invalid EXC_RETURN (low nibble 0x5).
    let mut cpu = cpu_with(&[0xDF00]);
    step(&mut cpu);
    cpu.r[14] = 0xFFFF_FFF5;
    assert!(matches!(
        cpu.step(&mut NoBus),
        Err(Stop::Unsupported { .. })
    ));
    // SYSRESETREQ.
    let mut cpu = cpu_with(&[0x6001]); // str r1, [r0]
    cpu.r[0] = 0xE000_ED0C;
    cpu.r[1] = 0x05FA_0004;
    assert!(matches!(
        cpu.step(&mut NoBus),
        Err(Stop::Unsupported { .. })
    ));
}

#[test]
fn division_by_zero_traps_only_with_div_0_trp() {
    // udiv r3, r1, r2 with r2 = 0
    let code = [0xFBB1, 0xF3F2];
    let mut cpu = cpu_with(&code);
    cpu.scs.ccr |= 1 << 4;
    cpu.scs.shcsr_enable = 1 << 18;
    step(&mut cpu);
    assert_eq!(cpu.ipsr as usize, USAGE_FAULT);
    assert_eq!(cpu.scs.cfsr, cfsr::DIVBYZERO);
}
