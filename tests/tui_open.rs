//! `o` in the TUI hands the terminal to a run config and takes it back.
//! Only testable in a real terminal: udo runs in a pseudo-terminal, keys go
//! in, and a terminal emulator (`vt100`) turns the output back into the
//! screen the user would see.
//!
//! The two risks it guards: keys typed into the script must all reach it
//! (the TUI's own key reader must not keep reading), and the TUI must come
//! back fully drawn.

use std::{
    fs,
    io::{Read, Write},
    os::unix::fs::PermissionsExt,
    path::Path,
    process::Command,
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};

const ROWS: u16 = 30;
const COLS: u16 = 100;
const TIMEOUT: Duration = Duration::from_secs(10);

/// `udo` (the TUI) running in a pseudo-terminal, its screen kept up to date
/// by a reader thread.
struct Tui {
    child: Box<dyn Child + Send + Sync>,
    writer: Box<dyn Write + Send>,
    screen: Arc<Mutex<vt100::Parser>>,
    _master: Box<dyn MasterPty + Send>,
}

impl Tui {
    fn start(root: &Path) -> Tui {
        let pair = native_pty_system()
            .openpty(PtySize {
                rows: ROWS,
                cols: COLS,
                pixel_width: 0,
                pixel_height: 0,
            })
            .unwrap();
        let mut cmd = CommandBuilder::new(env!("CARGO_BIN_EXE_udo"));
        cmd.env("UDO_ROOT", root);
        cmd.env("TERM", "xterm-256color");
        cmd.cwd(root);
        let child = pair.slave.spawn_command(cmd).unwrap();
        drop(pair.slave); // the child has it; EOF once it exits

        let screen = Arc::new(Mutex::new(vt100::Parser::new(ROWS, COLS, 0)));
        let mut reader = pair.master.try_clone_reader().unwrap();
        let feed = Arc::clone(&screen);
        thread::spawn(move || {
            let mut buf = [0u8; 4096];
            while let Ok(n) = reader.read(&mut buf) {
                if n == 0 {
                    break;
                }
                feed.lock().unwrap().process(&buf[..n]);
            }
        });
        Tui {
            child,
            writer: pair.master.take_writer().unwrap(),
            screen,
            _master: pair.master,
        }
    }

    /// Type `text` (`\r` is Enter).
    fn send(&mut self, text: &str) {
        self.writer.write_all(text.as_bytes()).unwrap();
        self.writer.flush().unwrap();
    }

    /// The screen as text, one line per row.
    fn contents(&self) -> String {
        self.screen.lock().unwrap().screen().contents()
    }

    /// Wait until the screen shows `text`; fails with the screen if not.
    fn wait_for(&self, text: &str) {
        self.wait_until(&format!("{text:?} on screen"), |s| s.contains(text));
    }

    /// Wait until `ok` holds for the screen (`what`: for the failure).
    fn wait_until(&self, what: &str, ok: impl Fn(&str) -> bool) {
        let deadline = Instant::now() + TIMEOUT;
        while !ok(&self.contents()) {
            assert!(Instant::now() < deadline, "no {what}:\n{}", self.contents());
            thread::sleep(Duration::from_millis(20));
        }
    }

    /// Wait until udo exits; its exit code.
    fn wait_exit(&mut self) -> u32 {
        let deadline = Instant::now() + TIMEOUT;
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                return status.exit_code();
            }
            assert!(Instant::now() < deadline, "udo did not quit:\n{}", self.contents());
            thread::sleep(Duration::from_millis(20));
        }
    }
}

impl Drop for Tui {
    /// A failed test must not leave udo running.
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}

/// A fresh root with one task, `lab 3`, that opens with the script `editor`
/// (`body` after the shebang). The TUI's cursor starts on that task.
fn root_with_editor(body: &str) -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    let udo = |args: &[&str]| {
        let out = Command::new(env!("CARGO_BIN_EXE_udo"))
            .env("UDO_ROOT", root.path())
            .args(args)
            .output()
            .unwrap();
        assert!(out.status.success(), "udo {args:?}: {out:?}");
    };
    udo(&["add", "task", "lab 3"]);
    udo(&["settings", "set", "/", "open_with=editor"]);

    fs::create_dir(root.path().join("run")).unwrap();
    add_script(root.path(), "editor", body);
    root
}

/// An executable `name` in `root`'s run folder: `body` after the shebang.
fn add_script(root: &Path, name: &str, body: &str) {
    let script = root.join("run").join(name);
    fs::write(&script, format!("#!/bin/sh\n{body}\n")).unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
}

/// Wait until `path` exists (a script ran); fails with the screen if not.
fn wait_for_file(tui: &Tui, path: &Path) {
    let deadline = Instant::now() + TIMEOUT;
    while !path.exists() {
        assert!(Instant::now() < deadline, "script did not run:\n{}", tui.contents());
        thread::sleep(Duration::from_millis(20));
    }
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
    let mut tui = Tui::start(root.path());
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
    assert!(tui.contents().contains("script ready"), "its output stays readable");

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
    let mut tui = Tui::start(root.path());
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
    add_script(root.path(), "other", &format!("touch '{}'", other_ran.display()));
    let mut tui = Tui::start(root.path());
    tui.wait_for("lab 3");

    tui.send("O");
    tui.wait_for("open lab 3 with");
    assert!(tui.contents().contains("> editor (default)"), "{}", tui.contents());
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
