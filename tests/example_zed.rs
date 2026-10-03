//! `examples/run/zed.sh` as installed, with a fake `zed` on `PATH` that
//! records its arguments and "closes the window" after a while. Tests
//! udo's part: the session per task, `--detach`, and `--new` (without it,
//! real Zed's `--wait` only returns when all of Zed quits; checked by hand
//! 2026-10-04, as is real Zed in general).

mod common;

use std::{
    fs,
    path::PathBuf,
    process::Command,
    thread,
    time::{Duration, Instant},
};

use common::{UDO, executable, install_example, ok, path_with, running_as, wait_until};

/// A root with `zed` as `open_with`, tasks `lab 1` / `lab 2` with folders,
/// and the fake `zed` in `bin/` (its calls in `zed.log`).
struct Setup {
    root: tempfile::TempDir,
    bin: PathBuf,
    log: PathBuf,
}

impl Setup {
    fn new() -> Setup {
        let root = tempfile::tempdir().unwrap();
        for task in ["lab 1", "lab 2"] {
            let dir = root.path().join(task);
            ok(
                root.path(),
                &["add", "task", task, "--dir", dir.to_str().unwrap()],
            );
        }
        install_example(root.path(), "zed");
        ok(root.path(), &["settings", "set", "/", "open_with=zed"]);
        let (bin, log) = (root.path().join("bin"), root.path().join("zed.log"));
        // `zed … DIR` (DIR last): lab 1's window closes after 1 s, lab 2's
        // after 4
        let body = format!(
            "echo \"$@\" >> '{}'\n\
             for dir; do :; done\n\
             case \"$dir\" in *'lab 1') sleep 1 ;; *) sleep 4 ;; esac",
            log.display()
        );
        executable(&bin, "zed", &body);
        Setup { root, bin, log }
    }

    /// `udo run TASK` with the fake first on `PATH`.
    fn open(&self, task: &str) {
        let out = Command::new(UDO)
            .env("UDO_ROOT", self.root.path())
            .env("PATH", path_with(&self.bin))
            .args(["run", task])
            .output()
            .unwrap();
        assert!(out.status.success(), "{out:?}");
    }

    fn logged(&self) -> String {
        fs::read_to_string(&self.log).unwrap_or_default()
    }

    fn running(&self) -> Option<(String, String, String)> {
        running_as(self.root.path())
    }

    fn wait_stopped(&self) {
        wait_until(
            "stopped",
            || self.running().is_none(),
            || format!("{:?}", self.running()),
        );
    }

    fn id(&self, task: &str) -> String {
        let out = ok(self.root.path(), &["show", task, "--json"]);
        let shown: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        shown["id"].as_str().unwrap().to_string()
    }
}

/// Returns at once; the session runs until the window closes, then the
/// helper stops it.
#[test]
fn opens_the_folder_and_times_until_the_window_closes() {
    let s = Setup::new();
    let owner = format!("zed:{}", s.id("lab 1"));

    let begin = Instant::now();
    s.open("lab 1");

    assert!(
        begin.elapsed() < Duration::from_secs(1),
        "waited {:?}",
        begin.elapsed()
    );
    assert_eq!(s.running(), Some(("lab 1".into(), "zed".into(), owner)));
    wait_until("zed called", || !s.logged().is_empty(), || s.logged());
    let dir = s.root.path().join("lab 1");
    // `--new`: its own window, so `--wait` ends with it, not with Zed
    assert_eq!(s.logged(), format!("--new --wait {}\n", dir.display()));
    s.wait_stopped();
}

/// One owner per task: lab 1's window closing does not end lab 2's
/// session (lab 2 took over; lab 1's stop is someone else's, a no-op).
#[test]
fn two_windows_do_not_stop_each_other() {
    let s = Setup::new();
    s.open("lab 1");
    s.open("lab 2"); // takes over

    thread::sleep(Duration::from_millis(2000)); // lab 1's window is closed
    assert_eq!(s.running().map(|r| r.0), Some("lab 2".into()));
    s.wait_stopped();
}

/// A container is opened for a task picked below it: the time goes to
/// that task, the window is the container's folder.
#[test]
fn a_container_opens_its_folder_for_the_picked_task() {
    let s = Setup::new();
    let cs = s.root.path().join("cs");
    ok(
        s.root.path(),
        &["add", "project", "cs", "--dir", cs.to_str().unwrap()],
    );
    ok(s.root.path(), &["add", "task", "cs/lab 1", "--no-dir"]);

    let out = Command::new(UDO)
        .env("UDO_ROOT", s.root.path())
        .env("PATH", path_with(&s.bin))
        .args(["run", "cs"]) // one open task: taken
        .output()
        .unwrap();

    assert!(out.status.success(), "{out:?}");
    assert_eq!(s.running().map(|r| r.0), Some("lab 1".into()));
    wait_until("zed called", || !s.logged().is_empty(), || s.logged());
    assert_eq!(s.logged(), format!("--new --wait {}\n", cs.display()));
    s.wait_stopped();
}
