//! `udo track run --detach`: starts udo's own binary again in the
//! background, so it is tested from outside, against the real binary and a
//! fresh root (`UDO_ROOT`).

use std::{
    path::Path,
    process::{Command, Output},
    thread::sleep,
    time::{Duration, Instant},
};

fn udo(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_udo"))
        .env("UDO_ROOT", root)
        .args(args)
        .output()
        .unwrap()
}

fn status(root: &Path) -> String {
    String::from_utf8(udo(root, &["status"]).stdout).unwrap()
}

/// `track run --detach --task <task> --source sh --owner sh:1 -- <cmd>`
fn detach(root: &Path, task: &str, cmd: &[&str]) -> Output {
    let flags = [
        "track", "run", "--detach", "--task", task, "--source", "sh", "--owner", "sh:1",
    ];
    let args: Vec<&str> = flags.iter().chain(&["--"]).chain(cmd).copied().collect();
    udo(root, &args)
}

/// Returns while the program still runs, the session already started (a
/// script's next command sees it); the helper stops it when the program
/// ends. `output()` waiting for the pipes also shows the helper does not
/// hold them.
#[test]
fn detach_returns_at_once_and_the_helper_stops_the_session() {
    let root = tempfile::tempdir().unwrap();
    assert!(udo(root.path(), &["add", "task", "x"]).status.success());

    let begin = Instant::now();
    let out = detach(root.path(), "x", &["sleep", "2"]);

    assert!(out.status.success(), "{out:?}");
    assert!(begin.elapsed() < Duration::from_secs(1), "waited {:?}", begin.elapsed());
    assert!(status(root.path()).contains("running: x"), "{}", status(root.path()));

    let deadline = Instant::now() + Duration::from_secs(10);
    while !status(root.path()).contains("no session running") {
        assert!(Instant::now() < deadline, "still running: {}", status(root.path()));
        sleep(Duration::from_millis(100));
    }
}

/// Mistakes show up before returning (the helper has no stderr).
#[test]
fn detach_reports_an_unknown_task_and_starts_nothing() {
    let root = tempfile::tempdir().unwrap();

    let out = detach(root.path(), "nope", &["sleep", "2"]);

    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.contains("not found: \"nope\""), "{stderr}");
    assert!(status(root.path()).contains("no session running"));
}
