//! Reverse execution: every position the timeline goes to, by any route,
//! has exactly the state a plain forward replay has there.
//!
//! The fixtures (see `tests/fixtures/README.md`) are interrupt recordings
//! made on QEMU: 94 nested interrupts whose keys need lookahead, and a
//! recording ended by a `HardFault`.

use backstep_emu::elf::Elf;
use backstep_emu::memory::WatchKind;
use backstep_emu::Cpu;
use backstep_replay::{parse, Recording, Stop, Timeline, Watchpoint};

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures");

fn load(name: &str) -> (Elf, Recording) {
    let elf = Elf::parse(std::fs::read(format!("{FIXTURES}/irq_ambiguous.elf")).unwrap()).unwrap();
    let rec = parse(&std::fs::read(format!("{FIXTURES}/{name}")).unwrap()).unwrap();
    (elf, rec)
}

/// The architectural state compared: r0-r15, xPSR and the special
/// registers.
fn state(cpu: &Cpu) -> [u32; 23] {
    let mut s = [0; 23];
    s[..16].copy_from_slice(&cpu.r);
    s[16] = cpu.xpsr();
    s[17] = cpu.msp();
    s[18] = cpu.psp();
    s[19] = u32::from(cpu.primask);
    s[20] = u32::from(cpu.basepri);
    s[21] = u32::from(cpu.faultmask);
    s[22] = cpu.control;
    s
}

/// Every state of the recording, by stepping forwards from the start.
fn forward_states(t: &mut Timeline<'_>) -> Vec<[u32; 23]> {
    t.seek(0).unwrap();
    let mut states = vec![state(t.cpu())];
    while t.step().unwrap() != Stop::End {
        states.push(state(t.cpu()));
    }
    states
}

#[test]
fn stepping_forwards_reaches_the_end_of_the_checked_replay() {
    let (elf, rec) = load("irq_ambiguous.bsr");
    let mut t = Timeline::new(&elf, &rec, 100_000_000).unwrap();
    assert_eq!(t.position(), 0);
    assert_eq!(
        t.cpu().r[15],
        rec.snapshot.cpu.pc & !1,
        "starts where the recording does"
    );
    let states = forward_states(&mut t);
    assert_eq!(states.len() as u64, t.end() + 1);
    assert_eq!(t.position(), t.end());
    assert_eq!(t.interrupts_delivered(), rec.irqs.len());
    assert_eq!(t.cpu().r[15], elf.symbol("bs_snapshot_take").unwrap() & !1);
    assert_eq!(t.step().unwrap(), Stop::End, "nothing after the end");
}

#[test]
fn every_route_to_a_position_gives_the_same_state() {
    let (elf, rec) = load("irq_ambiguous.bsr");
    let mut t = Timeline::new(&elf, &rec, 100_000_000).unwrap();
    let states = forward_states(&mut t);
    let end = t.end();
    // Jumps to pseudo-random positions, in both directions, and to the
    // checkpoints and their neighbours.
    let mut x = 12345u64;
    let mut targets: Vec<u64> = (0..300)
        .map(|_| {
            x = x.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
            (x >> 33) % (end + 1)
        })
        .collect();
    let i = t.interval();
    targets.extend([0, 1, i - 1, i, i + 1, 2 * i, end - 1, end, end + 50]);
    for p in targets {
        t.seek(p).unwrap();
        let p = p.min(end);
        assert_eq!(t.position(), p);
        assert_eq!(
            state(t.cpu()),
            states[usize::try_from(p).unwrap()],
            "seek to {p}"
        );
    }
    // Step backwards across interrupt entries and checkpoint boundaries.
    t.seek(2 * i + 300).unwrap();
    for _ in 0..700 {
        assert_eq!(t.step_back().unwrap(), Stop::Step);
        assert_eq!(
            state(t.cpu()),
            states[usize::try_from(t.position()).unwrap()]
        );
    }
    t.seek(0).unwrap();
    assert_eq!(t.step_back().unwrap(), Stop::Start);
    assert_eq!(t.position(), 0);
}

#[test]
fn breakpoints_stop_forwards_and_backwards_at_each_hit_in_turn() {
    let (elf, rec) = load("irq_ambiguous.bsr");
    let mut t = Timeline::new(&elf, &rec, 100_000_000).unwrap();
    let states = forward_states(&mut t);
    // The timer 0 stub: entered once per timer 0 interrupt.
    let stub = elf.symbol("timer0_stub").unwrap() & !1;
    let hits: Vec<u64> = (0..states.len())
        .filter(|&p| states[p][15] == stub)
        .map(|p| p as u64)
        .collect();
    assert!(hits.len() > 5, "{} timer 0 interrupts", hits.len());
    t.add_breakpoint(stub | 1); // Thumb bit ignored
    t.seek(0).unwrap();
    let mut never = || false;
    for &h in &hits {
        assert_eq!(t.run(&mut never).unwrap(), Stop::Breakpoint);
        assert_eq!(t.position(), h);
    }
    assert_eq!(t.run(&mut never).unwrap(), Stop::End);
    for &h in hits.iter().rev() {
        assert_eq!(t.run_back(&mut never).unwrap(), Stop::Breakpoint);
        assert_eq!(t.position(), h);
    }
    assert_eq!(t.run_back(&mut never).unwrap(), Stop::Start);
    assert_eq!(t.position(), 0);
    t.remove_breakpoint(stub);
    assert_eq!(t.run(&mut never).unwrap(), Stop::End);
}

#[test]
fn watchpoints_find_the_instruction_that_wrote() {
    let (elf, rec) = load("irq_ambiguous.bsr");
    let mut t = Timeline::new(&elf, &rec, 100_000_000).unwrap();
    // g_spin_count: written by the ambiguous wait loop only.
    let address = elf.symbol("g_spin_count").unwrap();
    let w = Watchpoint {
        address,
        len: 4,
        kind: WatchKind::Write,
    };
    t.add_watchpoint(w);
    let mut never = || false;
    // Forwards: stop after each write, which changed the value or not.
    let mut forward = Vec::new();
    loop {
        match t.run(&mut never).unwrap() {
            Stop::Watch(hit) => {
                assert!(hit.write);
                assert_eq!(hit.address, address);
                forward.push(t.position());
            }
            Stop::End => break,
            other => panic!("{other:?}"),
        }
    }
    assert!(forward.len() > 100, "{} writes", forward.len());
    // Backwards: stop before each write, at the store instruction.
    for &after in forward.iter().rev() {
        let Stop::Watch(hit) = t.run_back(&mut never).unwrap() else {
            panic!("expected a watch stop before position {after}");
        };
        assert_eq!(hit.address, address);
        assert_eq!(t.position(), after - 1, "stops before the store");
        let store = t.cpu().r[15];
        assert_eq!(
            t.step().unwrap(),
            Stop::Watch(hit),
            "the next instruction is the store"
        );
        assert_eq!(t.position(), after);
        t.step_back().unwrap();
        assert_eq!(t.cpu().r[15], store);
    }
    assert_eq!(t.run_back(&mut never).unwrap(), Stop::Start);
    // Reads of the same variable (the loop's load), with a read watch.
    t.remove_watchpoint(w);
    t.add_watchpoint(Watchpoint {
        kind: WatchKind::Read,
        ..w
    });
    match t.run(&mut never).unwrap() {
        Stop::Watch(hit) => assert!(!hit.write),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_crash_recording_runs_to_the_fault_and_back() {
    let (elf, rec) = load("irq_crash.bsr");
    let mut t = Timeline::new(&elf, &rec, 100_000_000).unwrap();
    let mut never = || false;
    assert_eq!(t.run(&mut never).unwrap(), Stop::End);
    assert_eq!(
        t.cpu().ipsr,
        3,
        "the recording ends inside the HardFault handler"
    );
    // Back to the HardFault entry, then one more step: the instruction
    // that faulted was the call through the corrupted pointer.
    let handler = elf.symbol("HardFault_Handler").unwrap() & !1;
    t.add_breakpoint(handler);
    assert_eq!(t.run_back(&mut never).unwrap(), Stop::Breakpoint);
    assert_eq!(t.cpu().ipsr, 3);
    t.step_back().unwrap();
    assert_eq!(t.cpu().ipsr, 0);
    assert_eq!(t.cpu().r[15], 0x0000_1000, "the corrupted function pointer");
}

#[test]
fn an_interrupt_callback_stops_a_long_run() {
    let (elf, rec) = load("irq_ambiguous.bsr");
    let mut t = Timeline::new(&elf, &rec, 100_000_000).unwrap();
    let mut polls = 0;
    let mut stop_soon = || {
        polls += 1;
        true
    };
    assert_eq!(t.run(&mut stop_soon).unwrap(), Stop::Interrupted);
    assert!(t.position() > 0 && t.position() < t.end());
    let mut stop_now = || true;
    assert_eq!(t.run_back(&mut stop_now).unwrap(), Stop::Interrupted);
    assert!(polls >= 1);
}
