//! `o` in the TUI hands the terminal to a run config and takes it back.
//! Only testable in a real terminal: udo runs in a pseudo-terminal, keys go
//! in, and a terminal emulator (`vt100`) turns the output back into the
//! screen the user would see.
//!
//! The two risks it guards: keys typed into the script must all reach it
//! (the TUI's own key reader must not keep reading), and the TUI must come
//! back fully drawn.

mod common;

use std::{fs, path::Path};

use common::{
    Pty, executable, install_example, ok, path_with, running_as, udo_command, wait_until,
};

/// A fresh root with one task, `lab 3`, that opens with the script `editor`
/// (`body` after the shebang). The TUI's cursor starts on that task.
fn root_with_editor(body: &str) -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    ok(root.path(), &["add", "task", "lab 3"]);
    ok(root.path(), &["settings", "set", "/", "open_with=editor"]);
    add_script(root.path(), "editor", body);
    root
}

/// An executable `name` in `root`'s run folder: `body` after the shebang.
fn add_script(root: &Path, name: &str, body: &str) {
    executable(&root.join("run"), name, body);
}

/// The TUI on `root`.
fn tui(root: &Path) -> Pty {
    Pty::udo(root, &[])
}

/// Wait until `path` exists (a script ran); fails with the screen if not.
fn wait_for_file(tui: &Pty, path: &Path) {
    wait_until("script run", || path.exists(), || tui.contents());
}

/// The script reads a line (every key must reach it), fails, and the TUI
/// asks for Enter before drawing over its output; then it is back, with a
/// toast, and still takes keys.
#[test]
fn o_hands_the_terminal_over_and_takes_it_back() {
    let out_dir = tempfile::tempdir().unwrap();
    let out = out_dir.path().join("typed");
    // five lines, one at a time: a second reader on the terminal would win
    // each line with even odds, so this misses it 1 time in 32
    let body = format!(
        "echo \"script ready for $UDO_TASK_NAME\"\n\
         for i in 1 2 3 4 5; do read line; echo \"$line\" >> '{}'; echo \"got $i\"; done\n\
         exit 1",
        out.display()
    );
    let root = root_with_editor(&body);
    let mut tui = tui(root.path());
    tui.wait_for("lab 3");

    tui.send("o");
    tui.wait_for("script ready for lab 3");
    for i in 1..=5 {
        tui.send(&format!("line {i}\r"));
        tui.wait_for(&format!("got {i}")); // a stolen line: never comes
    }

    tui.wait_for("press Enter to return");
    let typed = fs::read_to_string(&out).unwrap();
    assert_eq!(typed, "line 1\nline 2\nline 3\nline 4\nline 5\n"); // no key lost
    assert!(
        tui.contents().contains("script ready"),
        "its output stays readable"
    );

    tui.send("\r");
    // the toast, not the prompt line (which says the same after "[udo]")
    tui.wait_until("TUI with the toast", |s| {
        s.contains("editor exited with 1") && !s.contains("press Enter")
    });
    let screen = tui.contents();
    assert!(screen.contains("lab 3"), "{screen}");
    assert!(!screen.contains("script ready"), "leftovers:\n{screen}");

    tui.send("q");
    assert_eq!(tui.wait_exit(), 0);
}

/// A script that succeeds: no Enter prompt, straight back.
#[test]
fn a_script_that_succeeds_returns_without_asking() {
    let marker_dir = tempfile::tempdir().unwrap();
    let marker = marker_dir.path().join("ran");
    let root = root_with_editor(&format!("touch '{}'", marker.display()));
    let mut tui = tui(root.path());
    tui.wait_for("lab 3");

    tui.send("o");
    wait_for_file(&tui, &marker);

    tui.wait_for("lab 3");
    tui.send("q"); // keys reach the TUI again: no Enter needed first
    assert_eq!(tui.wait_exit(), 0);
    assert!(!tui.contents().contains("press Enter"));
}

/// `O`: the script picker is drawn (the default marked), `j` + Enter run
/// the other script, not `open_with`.
#[test]
fn shift_o_runs_the_picked_script() {
    let marks = tempfile::tempdir().unwrap();
    let (editor_ran, other_ran) = (marks.path().join("editor"), marks.path().join("other"));
    let root = root_with_editor(&format!("touch '{}'", editor_ran.display()));
    add_script(
        root.path(),
        "other",
        &format!("touch '{}'", other_ran.display()),
    );
    let mut tui = tui(root.path());
    tui.wait_for("lab 3");

    tui.send("O");
    tui.wait_for("open lab 3 with");
    assert!(
        tui.contents().contains("> editor (default)"),
        "{}",
        tui.contents()
    );
    tui.send("j");
    tui.wait_for("> other");
    tui.send("\r");

    wait_for_file(&tui, &other_ran);
    tui.wait_until("the TUI without the picker", |s| {
        s.contains("lab 3") && !s.contains("open lab 3 with")
    });
    assert!(!editor_ran.exists(), "the default ran instead");
    tui.send("q");
    assert_eq!(tui.wait_exit(), 0);
}

/// `t`, a name, Enter: the task is saved and `on_create` gets the terminal
/// with `UDO_EVENT=create`; then the TUI is back on the new task.
#[test]
fn a_create_form_runs_on_create() {
    let marks = tempfile::tempdir().unwrap();
    let ran = marks.path().join("setup");
    let root = root_with_editor("");
    let body = format!("echo \"$UDO_EVENT $UDO_NODE_NAME\" > '{}'", ran.display());
    add_script(root.path(), "setup", &body);
    ok(root.path(), &["settings", "set", "/", "on_create=setup"]);
    let mut tui = tui(root.path());
    tui.wait_for("lab 3");

    tui.send("t");
    tui.wait_for("setup"); // the row, naming the script
    tui.send("lab 4\r");

    wait_for_file(&tui, &ran);
    tui.wait_for("lab 4");
    assert_eq!(fs::read_to_string(&ran).unwrap(), "create lab 4\n");
    tui.send("q");
    assert_eq!(tui.wait_exit(), 0);
}

/// `o` with a GUI editor (`examples/run/zed.sh`, a fake `zed` on `PATH`):
/// `track run --detach` returns at once, so the TUI is back without
/// asking, the task timed by zed; the helper stops it when the "window"
/// closes.
#[test]
fn o_with_a_detaching_editor_comes_back_at_once() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("lab 3");
    ok(
        root.path(),
        &["add", "task", "lab 3", "--dir", dir.to_str().unwrap()],
    );
    install_example(root.path(), "zed");
    ok(root.path(), &["settings", "set", "/", "open_with=zed"]);
    let bin = root.path().join("bin");
    executable(&bin, "zed", "sleep 2");
    let mut cmd = udo_command(root.path(), &[]);
    cmd.env("PATH", path_with(&bin));
    let mut tui = Pty::start(cmd);
    tui.wait_for("lab 3");

    tui.send("o");

    let timed = || running_as(root.path()).map(|r| r.1);
    wait_until(
        "zed timing",
        || timed().as_deref() == Some("zed"),
        || tui.contents(),
    );
    tui.wait_until("the TUI, no prompt", |s| {
        s.contains("lab 3") && s.contains("· zed") && !s.contains("press Enter")
    });
    wait_until("stopped", || timed().is_none(), || tui.contents());
    tui.send("q");
    assert_eq!(tui.wait_exit(), 0);
}
