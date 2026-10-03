//! `--json`: stdout is the report and nothing else, so `udo … --json | jq`
//! works even when a script or program runs in between. Its stdout goes
//! to udo's stderr then (`run::Stdout`); without `--json` it stays on
//! stdout. Only visible from outside: the real binary, a fresh root.

use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::Path,
    process::{Command, Output},
};

const SAYS: &str = "the script says hi";

fn udo(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_udo"))
        .env("UDO_ROOT", root)
        .args(args)
        .output()
        .unwrap()
}

/// `udo ARGS` must succeed; its output.
fn ok(root: &Path, args: &[&str]) -> Output {
    let out = udo(root, args);
    assert!(out.status.success(), "udo {args:?}: {out:?}");
    out
}

/// A fresh root with the task `lab 1` and a script `talk` that prints
/// `SAYS`; `talk` is `on_create` and `open_with` on the root.
fn root_with_talk() -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    ok(root.path(), &["add", "task", "lab 1"]);
    let run = root.path().join("run");
    fs::create_dir(&run).unwrap();
    let script = run.join("talk");
    fs::write(&script, format!("#!/bin/sh\necho '{SAYS}'\n")).unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
    ok(root.path(), &["settings", "set", "/", "on_create=talk", "open_with=talk"]);
    root
}

/// The whole of stdout is one JSON value; fails with both streams if not.
fn json(out: &Output) -> serde_json::Value {
    serde_json::from_slice(&out.stdout).unwrap_or_else(|e| {
        panic!(
            "stdout is not JSON only ({e}):\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        )
    })
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

#[test]
fn add_json_keeps_the_scripts_output_off_stdout() {
    let root = root_with_talk();

    let out = ok(root.path(), &["add", "task", "lab 2", "--json"]);

    assert_eq!(json(&out)["ran"]["code"], 0);
    assert!(stderr(&out).contains(SAYS), "{out:?}");
}

#[test]
fn run_json_keeps_the_scripts_output_off_stdout() {
    let root = root_with_talk();

    let out = ok(root.path(), &["run", "lab 1", "--json"]);

    assert_eq!(json(&out)["script"], "talk");
    assert!(stderr(&out).contains(SAYS), "{out:?}");
}

#[test]
fn track_run_json_keeps_the_programs_output_off_stdout() {
    let root = root_with_talk();
    let args = [
        "track", "run", "--task", "lab 1", "--source", "sh", "--owner", "sh:1", "--json", "--",
        "echo", SAYS,
    ];

    let out = ok(root.path(), &args);

    assert_eq!(json(&out)["code"], 0);
    assert!(stderr(&out).contains(SAYS), "{out:?}");
}

/// Without `--json` nothing is moved: the script writes to stdout, as on
/// a terminal.
#[test]
fn without_json_the_script_writes_to_stdout() {
    let root = root_with_talk();

    let out = ok(root.path(), &["add", "task", "lab 2"]);

    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains(SAYS), "{out:?}");
    assert!(stdout.contains("added task lab 2"), "{out:?}");
    assert!(!stderr(&out).contains(SAYS), "{out:?}");
}
