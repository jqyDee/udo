//! `examples/run/idea.sh` as installed, with fakes on `PATH` for what it calls:
//! `pgrep` (is IntelliJ running?), `open` (start it), `idea` (the
//! launcher: records its arguments, then "closes the window" after a
//! while). Tests udo's part: the session per task, `--detach`, starting
//! IntelliJ first. Whether real IntelliJ is ready after `settle` seconds
//! stays a check by hand (docs/run-configs.md).

mod common;

use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
    time::{Duration, Instant},
};

use common::{UDO, executable, install_example, ok, path_with, running_as, wait_until};

/// A root with `idea` as `open_with`, tasks `lab 1` / `lab 2` with
/// folders, and the fakes in `bin/` (their records in `log/`).
struct Setup {
    root: tempfile::TempDir,
    bin: PathBuf,
    log: PathBuf,
}

impl Setup {
    /// `running`: what the fake `pgrep` says at first (later: always yes,
    /// IntelliJ came up).
    fn new(running: bool) -> Setup {
        let root = tempfile::tempdir().unwrap();
        let (bin, log) = (root.path().join("bin"), root.path().join("log"));
        fs::create_dir_all(&log).unwrap();
        for task in ["lab 1", "lab 2"] {
            let dir = root.path().join(task);
            ok(
                root.path(),
                &["add", "task", task, "--dir", dir.to_str().unwrap()],
            );
        }
        install_example(root.path(), "idea");
        ok(root.path(), &["settings", "set", "/", "open_with=idea"]);

        let up = log.join("up");
        if running {
            fs::write(&up, "").unwrap();
        }
        let log = log.display();
        // running once `open` was called (or from the start)
        executable(&bin, "pgrep", &format!("test -e '{}'", up.display()));
        executable(
            &bin,
            "open",
            &format!("echo \"$@\" >> '{log}/open'; touch '{}'", up.display()),
        );
        // `idea --wait DIR`: lab 1's window closes after 1 s, lab 2's after 4
        let body = format!(
            "echo \"$@\" >> '{log}/idea'\n\
             case \"$2\" in *'lab 1') sleep 1 ;; *) sleep 4 ;; esac"
        );
        executable(&bin, "idea", &body);
        Setup {
            bin,
            log: root.path().join("log"),
            root,
        }
    }

    /// `udo run TASK` with the fakes first on `PATH`.
    fn open(&self, task: &str) -> Output {
        let out = Command::new(UDO)
            .env("UDO_ROOT", self.root.path())
            .env("PATH", path_with(&self.bin))
            .args(["run", task])
            .output()
            .unwrap();
        assert!(out.status.success(), "{out:?}");
        out
    }

    fn logged(&self, what: &str) -> String {
        fs::read_to_string(self.log.join(what)).unwrap_or_default()
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
}

/// The task's id, for the owner (`idea:<id>`).
fn id(root: &Path, task: &str) -> String {
    let out = ok(root, &["show", task, "--json"]);
    let shown: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    shown["id"].as_str().unwrap().to_string()
}

/// IntelliJ running: returns at once, the session runs until the window
/// closes, then the helper stops it.
#[test]
fn opens_the_folder_and_times_until_the_window_closes() {
    let s = Setup::new(true);
    let owner = format!("idea:{}", id(s.root.path(), "lab 1"));

    let begin = Instant::now();
    s.open("lab 1");

    assert!(
        begin.elapsed() < Duration::from_secs(1),
        "waited {:?}",
        begin.elapsed()
    );
    assert_eq!(s.running(), Some(("lab 1".into(), "idea".into(), owner)));
    wait_until(
        "idea called",
        || !s.logged("idea").is_empty(),
        || s.logged("idea"),
    );
    let dir = s.root.path().join("lab 1");
    assert_eq!(s.logged("idea"), format!("--wait {}\n", dir.display()));
    assert_eq!(s.logged("open"), "", "running already: not started again");
    s.wait_stopped();
}

/// Not running: `open -a` first (else `idea --wait` would become the IDE
/// and wait for all of it), then as above. Takes the `settle` seconds.
#[test]
fn starts_intellij_first_when_it_is_not_running() {
    let s = Setup::new(false);

    s.open("lab 1");

    assert_eq!(s.logged("open"), "-a IntelliJ IDEA\n");
    assert_eq!(s.running().map(|r| r.1), Some("idea".into()));
    s.wait_stopped();
}

/// One owner per task: lab 1's window closing does not end lab 2's
/// session (lab 2 took over; lab 1's stop is someone else's, a no-op).
#[test]
fn two_projects_do_not_stop_each_other() {
    let s = Setup::new(true);
    s.open("lab 1");
    s.open("lab 2"); // takes over

    // lab 1's window closes after 1 s; lab 2's after 4
    std::thread::sleep(Duration::from_millis(2000));
    assert_eq!(s.running().map(|r| r.0), Some("lab 2".into()));
    s.wait_stopped();
}
