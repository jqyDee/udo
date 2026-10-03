//! Shared by the integration tests: the real `udo` binary on a fresh root
//! (`UDO_ROOT`), a pseudo-terminal for what needs one (the TUI, `tmux
//! attach`), the example scripts, fake programs on `PATH`, and waiting
//! for things that happen in the background.

// every test file compiles this module on its own and uses only a part
#![allow(dead_code)]

use std::{
    ffi::OsStr,
    fs,
    io::{Read, Write},
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};

pub const TIMEOUT: Duration = Duration::from_secs(10);
pub const UDO: &str = env!("CARGO_BIN_EXE_udo");

// ---------- udo ----------

/// `udo ARGS` on `root`, waited for.
pub fn udo(root: &Path, args: &[&str]) -> Output {
    Command::new(UDO)
        .env("UDO_ROOT", root)
        .args(args)
        .output()
        .unwrap()
}

/// `udo ARGS` must succeed; its output.
pub fn ok(root: &Path, args: &[&str]) -> Output {
    let out = udo(root, args);
    assert!(out.status.success(), "udo {args:?}: {out:?}");
    out
}

/// The running session (`udo status --json`: `running`), or None.
pub fn running(root: &Path) -> Option<serde_json::Value> {
    let out = ok(root, &["status", "--json"]);
    let status: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    Some(status["running"].clone()).filter(|r| !r.is_null())
}

/// `(task, source, owner)` of the running session, for comparing.
pub fn running_as(root: &Path) -> Option<(String, String, String)> {
    running(root).map(|r| {
        let text = |key: &str| r[key].as_str().unwrap().to_string();
        (text("task"), text("source"), text("owner"))
    })
}

// ---------- scripts ----------

/// The example run config `name` (`examples/run/<name>.sh`, found by its
/// stem like `Library` finds scripts) copied into `root`'s run folder
/// under its file name, as a user installs it: the tests run the example
/// itself, not a copy of it.
pub fn install_example(root: &Path, name: &str) -> PathBuf {
    let examples = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/run");
    let from = fs::read_dir(&examples)
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| p.file_stem() == Some(OsStr::new(name)))
        .unwrap_or_else(|| panic!("no example {name:?} in {}", examples.display()));
    let run = root.join("run");
    fs::create_dir_all(&run).unwrap();
    let to = run.join(from.file_name().unwrap());
    fs::copy(&from, &to).unwrap(); // keeps +x
    to
}

/// An executable `dir/name`: `body` after a `sh` shebang (a run script,
/// or a fake program for `PATH`).
pub fn executable(dir: &Path, name: &str, body: &str) -> PathBuf {
    fs::create_dir_all(dir).unwrap();
    let path = dir.join(name);
    fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    path
}

/// `PATH` with `first` in front of the current one: fakes in `first` win.
pub fn path_with(first: &Path) -> String {
    format!("{}:{}", first.display(), std::env::var("PATH").unwrap())
}

/// Is `program` on `PATH`? For tests that need a tool (tmux, shellcheck):
/// without it they say so and pass.
pub fn have(program: &str) -> bool {
    Command::new("sh")
        .args(["-c", &format!("command -v {program}")])
        .output()
        .is_ok_and(|o| o.status.success())
}

// ---------- waiting ----------

/// Wait until `ok` holds; fails with `what` and `context()` if not within
/// `TIMEOUT`. For what happens in the background (hooks, helpers).
pub fn wait_until(what: &str, mut ok: impl FnMut() -> bool, context: impl Fn() -> String) {
    let deadline = Instant::now() + TIMEOUT;
    while !ok() {
        assert!(Instant::now() < deadline, "no {what}:\n{}", context());
        thread::sleep(Duration::from_millis(50));
    }
}

// ---------- pseudo-terminal ----------

pub const ROWS: u16 = 30;
pub const COLS: u16 = 100;

/// A program in a pseudo-terminal, its screen kept up to date by a reader
/// thread (`vt100`). Killed when dropped: a failed test leaves nothing
/// running.
pub struct Pty {
    child: Box<dyn Child + Send + Sync>,
    writer: Box<dyn Write + Send>,
    screen: Arc<Mutex<vt100::Parser>>,
    _master: Box<dyn MasterPty + Send>,
}

impl Pty {
    /// `udo ARGS` (no ARGS: the TUI) on `root`, in `root`.
    pub fn udo(root: &Path, args: &[&str]) -> Pty {
        Pty::start(udo_command(root, args))
    }

    /// Start `cmd` in a fresh pseudo-terminal.
    pub fn start(cmd: CommandBuilder) -> Pty {
        let pair = native_pty_system()
            .openpty(PtySize {
                rows: ROWS,
                cols: COLS,
                pixel_width: 0,
                pixel_height: 0,
            })
            .unwrap();
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
        Pty {
            child,
            writer: pair.master.take_writer().unwrap(),
            screen,
            _master: pair.master,
        }
    }

    /// Type `text` (`\r` is Enter).
    pub fn send(&mut self, text: &str) {
        self.writer.write_all(text.as_bytes()).unwrap();
        self.writer.flush().unwrap();
    }

    /// The screen as text, one line per row.
    pub fn contents(&self) -> String {
        self.screen.lock().unwrap().screen().contents()
    }

    /// Wait until the screen shows `text`; fails with the screen if not.
    pub fn wait_for(&self, text: &str) {
        self.wait_until(&format!("{text:?} on screen"), |s| s.contains(text));
    }

    /// Wait until `ok` holds for the screen (`what`: for the failure).
    pub fn wait_until(&self, what: &str, ok: impl Fn(&str) -> bool) {
        wait_until(what, || ok(&self.contents()), || self.contents());
    }

    /// Wait until the program exits; its exit code.
    pub fn wait_exit(&mut self) -> u32 {
        let deadline = Instant::now() + TIMEOUT;
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                return status.exit_code();
            }
            assert!(
                Instant::now() < deadline,
                "did not exit:\n{}",
                self.contents()
            );
            thread::sleep(Duration::from_millis(20));
        }
    }

    /// Has the program exited?
    pub fn exited(&mut self) -> bool {
        self.child.try_wait().unwrap().is_some()
    }
}

impl Drop for Pty {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}

/// `udo ARGS` on `root`, in `root`, as a terminal program; add `env` to it
/// before `Pty::start`.
pub fn udo_command(root: &Path, args: &[&str]) -> CommandBuilder {
    let mut cmd = CommandBuilder::new(UDO);
    cmd.args(args.iter().map(OsStr::new));
    cmd.env("UDO_ROOT", root);
    cmd.env("TERM", "xterm-256color");
    cmd.cwd(root);
    cmd
}
