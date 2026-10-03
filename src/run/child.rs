//! Running a child with the terminal handed over: shared by `run::launch`
//! (scripts) and `udo track run` (any program). `Stdout` keeps a child's
//! output out of a `--json` report.

use std::{
    io,
    os::{fd::AsFd, unix::process::ExitStatusExt},
    process::{ExitStatus, Stdio},
};

/// Start `cmd` with the terminal inherited (no pipes: some programs then
/// think they are not on a terminal) and wait for it. Ctrl+C reaches the
/// child too (same process group) and it decides; udo ignores it meanwhile,
/// so it gets to what comes after (stopping a session, redrawing the TUI)
/// instead of dying under a running nvim. `Err`: the child could not start.
pub async fn wait(mut cmd: tokio::process::Command) -> io::Result<ExitStatus> {
    let mut child = cmd.spawn()?;
    loop {
        tokio::select! {
            status = child.wait() => return status,
            _ = tokio::signal::ctrl_c() => {} // handled now: no longer kills udo
        }
    }
}

/// The child's exit code; killed by a signal: 128 + the signal, like a
/// shell reports it.
pub fn exit_code(status: ExitStatus) -> i32 {
    status
        .code()
        .unwrap_or_else(|| 128 + status.signal().unwrap_or(0))
}

/// Where a child's stdout goes. Stdin and stderr stay the terminal either
/// way, so a script can still ask (`read`) and show its prompt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stdout {
    /// The terminal, like udo's own (the TUI, text output).
    Inherit,
    /// udo's stderr: udo's stdout is its `--json` report, and nothing
    /// else may be in it (`udo add --json | jq`). Still on screen, just
    /// not on the pipe.
    Stderr,
}

impl Stdout {
    /// `Stderr` with `--json`, else `Inherit`.
    pub fn for_json(json: bool) -> Self {
        if json { Self::Stderr } else { Self::Inherit }
    }

    /// Set `cmd`'s stdout accordingly. A copy of udo's stderr (`dup`), not
    /// fd 2 itself: `Stdio` closes what it is given after the spawn. Err:
    /// stderr cannot be duplicated (closed).
    pub fn apply(self, cmd: &mut tokio::process::Command) -> io::Result<()> {
        if self == Self::Stderr {
            let fd = io::stderr().as_fd().try_clone_to_owned()?;
            cmd.stdout(Stdio::from(fd));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sh(script: &str) -> tokio::process::Command {
        let mut cmd = tokio::process::Command::new("sh");
        cmd.args(["-c", script]);
        cmd
    }

    #[tokio::test]
    async fn the_code_comes_back() {
        let status = wait(sh("exit 3")).await.unwrap();

        assert_eq!(exit_code(status), 3);
    }

    #[tokio::test]
    async fn a_signal_is_128_plus_it() {
        let status = wait(sh("kill -TERM $$")).await.unwrap();

        assert_eq!(exit_code(status), 128 + 15);
    }

    #[tokio::test]
    async fn a_missing_program_is_an_error() {
        let cmd = tokio::process::Command::new("udo-no-such-program");

        assert_eq!(wait(cmd).await.unwrap_err().kind(), io::ErrorKind::NotFound);
    }

    #[test]
    fn json_sends_stdout_to_stderr() {
        assert_eq!(Stdout::for_json(true), Stdout::Stderr);
        assert_eq!(Stdout::for_json(false), Stdout::Inherit);
    }

    /// Redirected, the child still starts and its code comes back (where
    /// the output lands: `tests/cli_json.rs`, with the real binary).
    #[tokio::test]
    async fn a_child_with_stdout_on_stderr_still_runs() {
        let mut cmd = sh("echo to-stderr-now; exit 2");
        Stdout::Stderr.apply(&mut cmd).unwrap();

        assert_eq!(exit_code(wait(cmd).await.unwrap()), 2);
    }
}
