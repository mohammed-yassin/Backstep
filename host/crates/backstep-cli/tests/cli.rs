//! End-to-end tests of the `backstep` binary.

use std::process::Command;

fn backstep() -> Command {
    Command::new(env!("CARGO_BIN_EXE_backstep"))
}

#[test]
fn info_lists_both_targets() {
    let out = backstep().arg("info").output().expect("run backstep info");
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).expect("utf-8 output");
    assert!(stdout.starts_with(&format!("backstep {}", env!("CARGO_PKG_VERSION"))));
    assert!(stdout.contains("qemu-mps2-an386"));
    assert!(stdout.contains("nucleo-f411re"));
}

#[test]
fn crc32_of_check_string() {
    let dir = std::env::temp_dir().join(format!("backstep-cli-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create temp dir");
    let file = dir.join("check.txt");
    std::fs::write(&file, b"123456789").expect("write test file");

    let out = backstep()
        .arg("crc32")
        .arg(&file)
        .output()
        .expect("run backstep crc32");
    std::fs::remove_dir_all(&dir).ok();

    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).expect("utf-8 output");
    assert!(stdout.starts_with("CBF43926  "), "got: {stdout}");
}

#[test]
fn crc32_of_missing_file_fails_with_status_2() {
    let out = backstep()
        .args(["crc32", "this-file-does-not-exist.bin"])
        .output()
        .expect("run backstep crc32");
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8(out.stderr).expect("utf-8 output");
    assert!(stderr.contains("cannot read"));
}

#[test]
fn unknown_subcommand_is_rejected() {
    let out = backstep().arg("frobnicate").output().expect("run backstep");
    assert!(!out.status.success());
}

#[test]
fn inspect_and_replay_reject_bad_input_with_status_2() {
    let dir = std::env::temp_dir().join(format!("backstep-cli-rec-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create temp dir");
    let bad = dir.join("bad.bsr");
    std::fs::write(&bad, b"not a recording at all").expect("write test file");

    let inspect = backstep()
        .arg("inspect")
        .arg(&bad)
        .output()
        .expect("run inspect");
    let missing = backstep()
        .arg("inspect")
        .arg(dir.join("missing.bsr"))
        .output()
        .expect("run inspect");
    let replay = backstep()
        .arg("replay")
        .arg(&bad)
        .arg(&bad)
        .output()
        .expect("run replay");
    std::fs::remove_dir_all(&dir).ok();

    assert_eq!(inspect.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&inspect.stderr).contains("not a Backstep recording"));
    assert_eq!(missing.status.code(), Some(2));
    assert_eq!(replay.status.code(), Some(2));
}
