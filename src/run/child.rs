//! Running a child with the terminal handed over: shared by `run::launch`
//! (scripts) and `udo track run` (any program).

use std::{io, os::unix::process::ExitStatusExt, process::ExitStatus};

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
}
