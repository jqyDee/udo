//! `examples/run/nvim-tmux.sh` and `examples/run/tmux.sh` (a shell instead
//! of nvim, the same hooks) as installed, against a real tmux: the hook
//! rules from the spike (tmux 3.6a) checked end to end. Each test has its
//! own tmux server (`TMUX_TMPDIR`), never the user's; a fake `nvim` waits
//! for a line (Enter "quits" it); the client is `udo run` in a
//! pseudo-terminal (`tmux attach` needs one). Hooks run in the background,
//! so every check waits until it holds. Without tmux: skipped.

mod common;

use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
    time::Duration,
};

use common::{
    Pty, executable, have, install_example, ok, path_with, running_as, udo_command, wait_until,
};

/// A root with tasks `a` / `b` and the container `cs` (no tasks), the
/// example `example` as `open_with`, its own tmux server and the fake
/// `nvim`. The server is killed when dropped.
struct Setup {
    root: tempfile::TempDir,
    bin: PathBuf,
    tmux_dir: PathBuf,
}

impl Setup {
    /// None (and a note) without tmux: the test passes, says why.
    fn new(example: &str) -> Option<Setup> {
        if !have("tmux") {
            eprintln!("skipped: no tmux");
            return None;
        }
        let root = tempfile::tempdir().unwrap();
        let (bin, tmux_dir) = (root.path().join("bin"), root.path().join("tmux"));
        fs::create_dir_all(&tmux_dir).unwrap();
        for task in ["a", "b"] {
            ok(root.path(), &["add", "task", task]);
        }
        let cs = root.path().join("cs");
        ok(
            root.path(),
            &["add", "project", "cs", "--dir", cs.to_str().unwrap()],
        );
        install_example(root.path(), example);
        let open_with = format!("open_with={example}");
        ok(root.path(), &["settings", "set", "/", &open_with]);
        executable(&bin, "nvim", "read _"); // Enter quits it
        let s = Setup {
            root,
            bin,
            tmux_dir,
        };
        // the server first, without the user's config (`-f /dev/null`): a
        // tmux.conf may restore sessions or set hooks of its own. The
        // script's calls then find it running and load nothing.
        s.tmux_ok(&[
            "-f",
            "/dev/null",
            "new-session",
            "-d",
            "-s",
            "keep",
            "sleep 600",
        ]);
        // `tmux`'s sessions run a shell: a plain one, not the user's (its
        // rc files may change PATH or print things)
        s.tmux_ok(&["set-option", "-g", "default-shell", "/bin/sh"]);
        Some(s)
    }

    fn root(&self) -> &Path {
        self.root.path()
    }

    /// `tmux ARGS` on this test's server.
    fn tmux(&self, args: &[&str]) -> Output {
        Command::new("tmux")
            .env("TMUX_TMPDIR", &self.tmux_dir)
            .env("PATH", path_with(&self.bin))
            .env_remove("TMUX")
            .args(args)
            .output()
            .unwrap()
    }

    /// `tmux ARGS` must succeed; its stdout.
    fn tmux_ok(&self, args: &[&str]) -> String {
        let out = self.tmux(args);
        assert!(out.status.success(), "tmux {args:?}: {out:?}");
        String::from_utf8(out.stdout).unwrap()
    }

    /// `udo run TASK` outside tmux, in a terminal: attaches (and blocks).
    fn attach(&self, task: &str) -> Pty {
        let mut cmd = udo_command(self.root(), &["run", task]);
        cmd.env("TMUX_TMPDIR", &self.tmux_dir);
        cmd.env("PATH", path_with(&self.bin));
        cmd.env_remove("TMUX");
        Pty::start(cmd)
    }

    /// `udo run TASK` as from inside tmux (`TMUX` set): switches the
    /// client over and returns.
    fn switch_to(&self, task: &str) {
        let socket = self.tmux_ok(&["display-message", "-p", "#{socket_path}"]);
        let out = Command::new(common::UDO)
            .env("UDO_ROOT", self.root())
            .env("TMUX_TMPDIR", &self.tmux_dir)
            .env("PATH", path_with(&self.bin))
            .env("TMUX", format!("{},0,0", socket.trim()))
            .args(["run", task])
            .output()
            .unwrap();
        assert!(out.status.success(), "{out:?}");
    }

    /// The session the example makes for `node` (a task, or a container
    /// opened without one).
    fn session(&self, node: &str) -> String {
        let out = ok(self.root(), &["show", node, "--json"]);
        let shown: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        format!("udo-{}", shown["id"].as_str().unwrap())
    }

    /// What `udo status` should say while `task`'s session is timed.
    fn timed(&self, task: &str) -> Option<(String, String, String)> {
        let owner = format!("tmux:{}", self.session(task));
        Some((task.into(), "tmux".into(), owner))
    }

    fn running(&self) -> Option<(String, String, String)> {
        running_as(self.root())
    }

    /// Wait until `udo status` says `want`.
    fn wait_running(&self, want: Option<(String, String, String)>) {
        let what = format!("running = {want:?}");
        wait_until(
            &what,
            || self.running() == want,
            || format!("{:?}", self.running()),
        );
    }

    /// The one client's name (for `switch-client -c`).
    fn client(&self) -> String {
        wait_until("a client", || !self.clients().is_empty(), String::new);
        self.clients().lines().next().unwrap().to_string()
    }

    fn clients(&self) -> String {
        String::from_utf8(self.tmux(&["list-clients", "-F", "#{client_name}"]).stdout).unwrap()
    }
}

impl Drop for Setup {
    fn drop(&mut self) {
        let _ = self.tmux(&["kill-server"]);
    }
}

#[test]
fn attaching_starts_and_detaching_stops() {
    let Some(s) = Setup::new("nvim-tmux") else {
        return;
    };

    let mut client = s.attach("a");
    s.wait_running(s.timed("a"));

    s.tmux_ok(&["detach-client", "-s", &format!("={}", s.session("a"))]);
    s.wait_running(None);
    assert_eq!(client.wait_exit(), 0, "udo run returns after the detach");
}

/// Quitting nvim ends the session and its own hooks: the global
/// session-closed stops the timer.
#[test]
fn quitting_nvim_stops() {
    let Some(s) = Setup::new("nvim-tmux") else {
        return;
    };
    let _client = s.attach("a");
    s.wait_running(s.timed("a"));

    // a pane target: `=name:` (`=name` alone is no pane)
    s.tmux_ok(&["send-keys", "-t", &format!("={}:", s.session("a")), "Enter"]);

    s.wait_running(None);
}

/// Inside tmux, opening another task switches the client: its session's
/// hook takes the timer over.
#[test]
fn switching_to_another_task_times_that_one() {
    let Some(s) = Setup::new("nvim-tmux") else {
        return;
    };
    let _client = s.attach("a");
    s.wait_running(s.timed("a"));

    s.switch_to("b");

    s.wait_running(s.timed("b"));
}

/// Switching to a session that is not udo's fires nothing on the one left:
/// the global hook stops `#{client_last_session}`.
#[test]
fn switching_to_a_foreign_session_stops() {
    let Some(s) = Setup::new("nvim-tmux") else {
        return;
    };
    let _client = s.attach("a");
    s.wait_running(s.timed("a"));
    s.tmux_ok(&["new-session", "-d", "-s", "notes", "sleep 600"]);

    s.tmux_ok(&["switch-client", "-c", &s.client(), "-t", "=notes"]);

    s.wait_running(None);
}

/// A timer started by hand stays: attaching does not take it over, the
/// detach (another owner) does not stop it (stage 1, row 4).
#[test]
fn a_manual_timer_is_left_alone() {
    let Some(s) = Setup::new("nvim-tmux") else {
        return;
    };
    ok(s.root(), &["start", "a"]);
    let manual = Some(("a".into(), "manual".into(), "manual".into()));

    // nothing may change, so there is nothing to wait for: give the
    // background hooks a second after each step, then look
    let mut client = s.attach("a");
    s.client();
    std::thread::sleep(Duration::from_secs(1));
    assert_eq!(s.running(), manual, "attaching took it over");

    s.tmux_ok(&["detach-client", "-s", &format!("={}", s.session("a"))]);
    client.wait_exit();
    std::thread::sleep(Duration::from_secs(1));
    assert_eq!(s.running(), manual, "the detach stopped it");
}

/// Our global hooks sit at index 77: a hook of the user's own stays.
#[test]
fn the_users_own_hooks_stay() {
    let Some(s) = Setup::new("nvim-tmux") else {
        return;
    };
    s.tmux_ok(&["new-session", "-d", "-s", "mine", "sleep 600"]);
    s.tmux_ok(&["set-hook", "-g", "client-detached", "run-shell 'true'"]);
    let mut client = s.attach("a");
    s.wait_running(s.timed("a"));

    let hooks = s.tmux_ok(&["show-hooks", "-g", "client-detached"]);

    assert!(
        hooks.contains("client-detached[0] run-shell true"),
        "{hooks}"
    );
    assert!(hooks.contains("client-detached[77]"), "{hooks}");
    s.tmux_ok(&["detach-client", "-s", &format!("={}", s.session("a"))]);
    client.wait_exit();
}

/// A container opened without a task: a session of its own, untimed; going
/// there from a task's session stops that one's timer.
#[test]
#[ignore = "needs `udo run CONTAINER` without a task (#52, CLI part)"]
fn a_container_without_a_task_gets_an_untimed_session() {
    let Some(s) = Setup::new("nvim-tmux") else {
        return;
    };
    let _client = s.attach("a");
    s.wait_running(s.timed("a"));

    s.switch_to("cs");

    s.wait_running(None);
    s.tmux_ok(&["has-session", "-t", &format!("={}", s.session("cs"))]);
}

// ---------- tmux.sh: a shell instead of nvim ----------

#[test]
fn tmux_attaching_starts_and_detaching_stops() {
    let Some(s) = Setup::new("tmux") else { return };

    let mut client = s.attach("a");
    s.wait_running(s.timed("a"));

    s.tmux_ok(&["detach-client", "-s", &format!("={}", s.session("a"))]);
    s.wait_running(None);
    assert_eq!(client.wait_exit(), 0, "udo run returns after the detach");
}

/// What the user starts in the shell ends, the session (and its timer)
/// stays; ending the shell ends it, and the global session-closed stops
/// the timer.
#[test]
fn tmux_the_session_outlives_a_program_in_it() {
    let Some(s) = Setup::new("tmux") else { return };
    let _client = s.attach("a");
    s.wait_running(s.timed("a"));
    let pane = format!("={}:", s.session("a"));

    // a program that ends on Enter, like quitting nvim
    s.tmux_ok(&["send-keys", "-t", &pane, "sh -c 'read _'", "Enter"]);
    s.tmux_ok(&["send-keys", "-t", &pane, "Enter"]);
    std::thread::sleep(Duration::from_secs(1));
    s.tmux_ok(&["has-session", "-t", &format!("={}", s.session("a"))]);
    assert_eq!(s.running(), s.timed("a"), "still timed");

    s.tmux_ok(&["send-keys", "-t", &pane, "exit", "Enter"]);
    s.wait_running(None);
}

/// Opening the task again finds its session (by id), not a second one.
#[test]
fn tmux_opening_again_reuses_the_session() {
    let Some(s) = Setup::new("tmux") else { return };
    let mut client = s.attach("a");
    s.wait_running(s.timed("a"));
    s.tmux_ok(&["detach-client", "-s", &format!("={}", s.session("a"))]);
    client.wait_exit();
    s.wait_running(None);

    let _client = s.attach("a");

    s.wait_running(s.timed("a"));
    let sessions = s.tmux_ok(&["list-sessions", "-F", "#{session_name}"]);
    let ours = sessions.lines().filter(|l| l.starts_with("udo-")).count();
    assert_eq!(ours, 1, "{sessions}");
}

#[test]
#[ignore = "needs `udo run CONTAINER` without a task (#52, CLI part)"]
fn tmux_a_container_without_a_task_gets_an_untimed_session() {
    let Some(s) = Setup::new("tmux") else { return };
    let _client = s.attach("a");
    s.wait_running(s.timed("a"));

    s.switch_to("cs");

    s.wait_running(None);
    s.tmux_ok(&["has-session", "-t", &format!("={}", s.session("cs"))]);
}
