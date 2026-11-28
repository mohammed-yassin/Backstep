//! Queries on the checked-in fixture recording of `backstep-replay`
//! (`tests/fixtures`): its ELF has no debug information, so variables
//! come from the symbol table. The DWARF side is tested end to end by
//! `scripts/why-test.py`.

use backstep_emu::elf::Elf;
use backstep_query::{events, why, Context, DebugInfo, EventKind};
use backstep_replay::{parse, Timeline};

const FIXTURES: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../backstep-replay/tests/fixtures"
);

fn elf() -> Elf {
    Elf::parse(std::fs::read(format!("{FIXTURES}/irq_ambiguous.elf")).unwrap()).unwrap()
}

#[test]
fn why_finds_every_write_and_the_last() {
    let elf = elf();
    let rec = parse(&std::fs::read(format!("{FIXTURES}/irq_ambiguous.bsr")).unwrap()).unwrap();
    let debug = DebugInfo::new(&elf);
    let v = debug.variable("g_spin_count").unwrap();
    assert_eq!(v.address, elf.symbol("g_spin_count").unwrap());
    assert_eq!(v.size, 4, "no debug information: 4 bytes assumed");
    let mut t = Timeline::new(&elf, &rec, 100_000_000).unwrap();
    let w = why(&mut t, &debug, &v, None).unwrap();
    let last = w.last_write.clone().unwrap();
    assert_eq!(
        last.context,
        Context::Thread,
        "only the main code writes it"
    );
    assert_eq!(w.value, last.after);
    assert!(w.writes.len() > 100, "{} writes", w.writes.len());
    assert!(w.writes.windows(2).all(|p| p[0].position < p[1].position));
    assert!(
        w.writes.windows(2).all(|p| p[0].after == p[1].before),
        "the values chain"
    );
    assert_eq!(w.lost_updates.len(), 0);
    assert_eq!(
        last.at.function.as_deref(),
        Some("main"),
        "from the symbol table"
    );
    // At the position of the first write, nothing is written yet.
    let first = w.writes[0].position;
    let early = why(&mut t, &debug, &v, Some(first)).unwrap();
    assert!(early.last_write.is_none());
    assert_eq!(early.value, w.writes[0].before);
    // Errors, and raw addresses.
    assert!(debug.variable("no_such_thing").is_err());
    assert!(debug.variable("g_spin_count.x").is_err());
    let raw = debug.variable(&format!("{:#x}:2", v.address)).unwrap();
    assert_eq!((raw.address, raw.size), (v.address, 2));
}

#[test]
fn the_timeline_has_every_recorded_interrupt() {
    let elf = elf();
    let rec = parse(&std::fs::read(format!("{FIXTURES}/irq_ambiguous.bsr")).unwrap()).unwrap();
    let debug = DebugInfo::new(&elf);
    let mut t = Timeline::new(&elf, &rec, 100_000_000).unwrap();
    let ev = events(&mut t, &debug).unwrap();
    assert_eq!(ev.first().unwrap().kind, EventKind::Start);
    assert_eq!(ev.last().unwrap().kind, EventKind::End);
    assert_eq!(ev.last().unwrap().position, t.end());
    let entries: Vec<(u32, bool, bool)> = ev
        .iter()
        .filter_map(|e| match &e.kind {
            EventKind::Entry {
                exception,
                fault,
                nested,
                ..
            } => Some((*exception, *fault, *nested)),
            _ => None,
        })
        .collect();
    assert_eq!(entries.len(), rec.irqs.len());
    assert!(entries.iter().all(|e| !e.1), "no faults");
    let mut a: Vec<u32> = entries.iter().map(|e| e.0).collect();
    let mut b: Vec<u32> = rec.irqs.iter().map(|i| i.exception).collect();
    a.sort_unstable();
    b.sort_unstable();
    assert_eq!(a, b, "the same interrupts");
    assert!(entries.iter().any(|e| e.2), "some nested");
}

#[test]
fn a_crash_shows_as_a_hardfault_with_its_cause() {
    let elf = elf();
    let crash = parse(&std::fs::read(format!("{FIXTURES}/irq_crash.bsr")).unwrap()).unwrap();
    let debug = DebugInfo::new(&elf);
    let mut t = Timeline::new(&elf, &crash, 100_000_000).unwrap();
    let ev = events(&mut t, &debug).unwrap();
    let fault = ev
        .iter()
        .find(|e| matches!(e.kind, EventKind::Entry { fault: true, .. }))
        .unwrap();
    let EventKind::Entry { name, causes, .. } = &fault.kind else {
        unreachable!()
    };
    assert_eq!(name, "HardFault");
    assert_eq!(causes, &["invstate".to_string()]);
    assert_eq!(fault.at.address, 0x1000, "the bad call target");
}
