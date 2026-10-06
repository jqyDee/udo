//! A failed estimate row (#47) never fails the command: the action stands,
//! exit 0, and the warning goes to stderr (`--json` stdout stays clean).
//! Only visible from outside: the real binary on a `udo.db` whose
//! `estimates` table is gone (migrated by `user_version`, so it opens).

mod common;

use common::{ok, udo};

/// A fresh root with the `estimate` setting (so a new task has a row to
/// write) and a broken estimate history. Keep the `TempDir` alive.
fn broken_root() -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    ok(root.path(), &["settings", "set", "/", "estimate=1h30"]); // creates udo.db
    let db = rusqlite::Connection::open(root.path().join("udo.db")).unwrap();
    db.execute_batch("DROP TABLE estimates").unwrap();
    root
}

fn stderr(out: &std::process::Output) -> String {
    String::from_utf8(out.stderr.clone()).unwrap()
}

#[test]
fn a_failed_row_warns_on_stderr_and_exits_0() {
    let root = broken_root();

    let out = udo(root.path(), &["add", "task", "lab 1"]);

    assert!(out.status.success(), "{out:?}");
    assert!(
        stderr(&out).starts_with("warning: estimate not recorded: "),
        "{}",
        stderr(&out)
    );
    ok(root.path(), &["show", "lab 1"]); // the task is there
}

#[test]
fn json_stdout_stays_clean() {
    let root = broken_root();

    let out = udo(root.path(), &["add", "task", "lab 1", "--json"]);

    assert!(out.status.success(), "{out:?}");
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(json.is_object(), "{json}");
    assert!(stderr(&out).contains("warning: estimate not recorded: "));
}
