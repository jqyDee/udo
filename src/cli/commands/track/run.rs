//! `udo track run [--detach] --task NODE --source NAME --owner OWNER -- CMD
//! [ARGS…]`: time a task while a program runs (`idea --wait`, `zed --wait`,
//! nvim in the foreground). `--detach`: start the session, hand the waiting
//! to a helper in the background (udo itself again, `--started`) and
//! return at once.

use std::{
    fmt, io,
    path::Path,
    process::{ExitStatus, Stdio},
};

use serde::Serialize;

use crate::{
    Res,
    cli::{commands::timer::SessionLine, report::Report, resolve::resolve},
    core::Core,
    model::{
        sessions::{Owner, SessionSource},
        time::Time,
    },
};

#[derive(clap::Args)]
pub struct RunArgs {
    /// The task, as NODE everywhere (scripts: id:$UDO_TASK_ID)
    #[arg(long)]
    pub task: String,
    /// What kind of program: a-z, 0-9, - (idea, zed, …)
    #[arg(long)]
    pub source: SessionSource,
    /// Which instance may stop it, e.g. idea:<task id>; not "manual"
    #[arg(long)]
    pub owner: Owner,
    /// Return at once; a helper in the background waits for the program
    /// and stops the session (GUI editors)
    #[arg(long)]
    pub detach: bool,
    /// Internal, the `--detach` helper: the session is already started
    #[arg(long, hide = true, conflicts_with = "detach")]
    pub started: bool,
    /// The program and its arguments, after `--`
    #[arg(last = true, required = true)]
    pub cmd: Vec<String>,
}

/// What `track run` did: the session it ended (`None`: taken over
/// meanwhile, e.g. `s` on another task) and the child's exit code.
#[derive(Serialize)]
pub struct Ran {
    pub session: Option<SessionLine>,
    pub code: i32,
}

/// What `track run --detach` did: the session it started, and the helper
/// that waits for the program and then stops it.
#[derive(Serialize)]
pub struct Detached {
    pub session: SessionLine,
    pub helper: u32,
}

/// Time `args.task` while `args.cmd` runs. `now` is asked at the start and
/// again once the child has ended. The session is stopped even if the
/// child could not start (then the error is returned): none is left open.
/// `args.started` (the helper): the session runs already, only wait and
/// stop.
pub async fn run(core: &mut Core, cwd: &Path, now: impl Fn() -> Time, args: &RunArgs) -> Res<Ran> {
    if !args.started {
        let path = resolve(core.tree(), Some(&args.task), cwd)?;
        core.track_start(&path, args.source.clone(), args.owner.clone(), now())
            .await?;
    }

    let status = wait_for(&args.cmd).await;
    let end = now();
    let stopped = core.track_stop(&args.owner, end).await?; // before `status?`
    let code = exit_code(status?);
    Ok(Ran {
        session: stopped.as_ref().map(|s| SessionLine::of(s, end)),
        code,
    })
}

/// `--detach`: start the session here, so mistakes (unknown or done task,
/// `--owner manual`) are reported and nothing races a script's next
/// command; then leave the waiting to a helper and return. The helper
/// cannot be started: the session is stopped again.
pub async fn detach(core: &mut Core, cwd: &Path, now: Time, args: &RunArgs) -> Res<Detached> {
    let path = resolve(core.tree(), Some(&args.task), cwd)?;
    let session = core
        .track_start(&path, args.source.clone(), args.owner.clone(), now)
        .await?;
    // by ID: the helper times exactly this task, even if renamed meanwhile
    let task = format!("id:{}", session.task.id);
    match spawn_helper(&task, args) {
        Ok(helper) => Ok(Detached {
            session: SessionLine::of(&session, now),
            helper,
        }),
        Err(e) => {
            // ended at its own start: 0 minutes, so the store drops it
            // (`EndBeforeStart`, expected here)
            let _ = core.track_stop(&args.owner, now).await;
            Err(e)
        }
    }
}

/// `udo track run --started …` in a new session (`setsid`: no terminal, so
/// closing one does not take it along), stdin / stdout / stderr on
/// `/dev/null` (a script's `$(…)` does not wait for it). The program runs
/// in the current folder, like without `--detach`. Returns its process ID.
fn spawn_helper(task: &str, args: &RunArgs) -> Res<u32> {
    use std::os::unix::process::CommandExt;

    let mut helper = std::process::Command::new(std::env::current_exe()?);
    helper
        .args(["track", "run", "--started", "--task", task])
        .args(["--source", args.source.as_str()])
        .args(["--owner", args.owner.as_str(), "--"])
        .args(&args.cmd)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    // SAFETY: runs between fork and exec; `setsid` is async-signal-safe and
    // nothing else happens there
    unsafe {
        helper.pre_exec(|| {
            if libc::setsid() == -1 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let child = helper
        .spawn()
        .map_err(|e| format!("cannot start the background helper: {e}"))?;
    Ok(child.id())
}

/// Run `cmd` with the terminal inherited (no pipes: some programs then
/// think they are not on a terminal) and wait for it. Ctrl+C reaches the
/// child too (same process group) and it decides; udo ignores it, so it
/// still gets to stop the session.
async fn wait_for(cmd: &[String]) -> Res<ExitStatus> {
    let (program, args) = cmd.split_first().ok_or("no command after --")?;
    let mut child = tokio::process::Command::new(program)
        .args(args)
        .spawn()
        .map_err(|e| format!("cannot start {program:?}: {e}"))?;
    loop {
        tokio::select! {
            status = child.wait() => return Ok(status?),
            _ = tokio::signal::ctrl_c() => {} // handled now: no longer kills udo
        }
    }
}

/// The child's exit code; killed by a signal: 128 + the signal, like a
/// shell reports it.
fn exit_code(status: ExitStatus) -> i32 {
    use std::os::unix::process::ExitStatusExt;
    status
        .code()
        .unwrap_or_else(|| 128 + status.signal().unwrap_or(0))
}

impl fmt::Display for Ran {
    fn fmt(&self, _: &mut fmt::Formatter<'_>) -> fmt::Result {
        Ok(()) // quiet: the child's output is what counts
    }
}

impl Report for Ran {}

impl fmt::Display for Detached {
    fn fmt(&self, _: &mut fmt::Formatter<'_>) -> fmt::Result {
        Ok(()) // quiet, like every `track` command
    }
}

impl Report for Detached {}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;
    use crate::{
        model::sessions::SessionStore,
        test_util::{at, core}, // core: disk_tree, root: [a, ws: [b]]
    };

    /// `track run --task a --source sh --owner sh:1 -- <cmd>`
    fn args(cmd: &[&str]) -> RunArgs {
        RunArgs {
            task: "a".into(),
            source: "sh".parse().unwrap(),
            owner: "sh:1".parse().unwrap(),
            detach: false,
            started: false,
            cmd: cmd.iter().map(|s| s.to_string()).collect(),
        }
    }

    // `detach` itself starts udo's own binary again: tested from outside,
    // in `tests/track_detach.rs`.

    /// A clock that says 14:00, then 15:00, 16:00, …: start and end differ.
    fn ticking() -> impl Fn() -> Time {
        let hour = Cell::new(14);
        move || {
            let now = at(hour.get(), 0);
            hour.set(hour.get() + 1);
            now
        }
    }

    #[tokio::test]
    async fn the_session_lasts_as_long_as_the_child() {
        let (tmp, mut core) = core().await;

        let ran = run(&mut core, tmp.path(), ticking(), &args(&["true"]))
            .await
            .unwrap();

        assert_eq!(ran.code, 0);
        let session = ran.session.unwrap();
        assert_eq!((session.task.as_str(), session.minutes), ("a", 60));
        assert_eq!((session.started, session.ended), (at(14, 0), Some(at(15, 0))));
        assert_eq!((session.source.as_str(), session.owner.as_str()), ("sh", "sh:1"));
        assert_eq!(core.sessions().running().await.unwrap(), None);
    }

    #[tokio::test]
    async fn the_childs_exit_code_comes_back() {
        let (tmp, mut core) = core().await;

        let ran = run(&mut core, tmp.path(), ticking(), &args(&["sh", "-c", "exit 3"]))
            .await
            .unwrap();

        assert_eq!(ran.code, 3);
        assert!(ran.session.is_some()); // a failing child is still timed
    }

    #[tokio::test]
    async fn a_child_killed_by_a_signal_is_128_plus_it() {
        let (tmp, mut core) = core().await;

        let ran = run(&mut core, tmp.path(), ticking(), &args(&["sh", "-c", "kill -TERM $$"]))
            .await
            .unwrap();

        assert_eq!(ran.code, 128 + 15);
    }

    /// The session was started before the spawn failed: stopped again.
    #[tokio::test]
    async fn a_missing_program_is_an_error_and_leaves_nothing_running() {
        let (tmp, mut core) = core().await;

        let err = run(&mut core, tmp.path(), ticking(), &args(&["udo-no-such-program"]))
            .await
            .err()
            .unwrap();

        assert!(
            err.to_string()
                .contains("cannot start \"udo-no-such-program\""),
            "{err}"
        );
        assert_eq!(core.sessions().running().await.unwrap(), None);
    }

    /// Nothing is started for a task that cannot be timed.
    #[tokio::test]
    async fn a_task_that_cannot_be_timed_runs_nothing() {
        let (tmp, mut core) = core().await;
        let marker = tmp.path().join("ran");
        let touch = format!("touch {}", marker.display());
        let mut done = args(&["sh", "-c", &touch]);
        core.set_done(&[0], true, at(13, 0)).await.unwrap();

        assert!(run(&mut core, tmp.path(), ticking(), &done).await.is_err());
        done.task = "ws".into(); // a container
        assert!(run(&mut core, tmp.path(), ticking(), &done).await.is_err());

        assert!(!marker.exists());
    }

    /// The helper (`--started`): the session runs already; it only waits
    /// and stops, ending the one the parent started.
    #[tokio::test]
    async fn started_only_waits_and_stops() {
        let (tmp, mut core) = core().await;
        let mut helper = args(&["true"]);
        let started = core
            .track_start(&[0], helper.source.clone(), helper.owner.clone(), at(13, 0))
            .await
            .unwrap();
        helper.started = true;

        let ran = run(&mut core, tmp.path(), ticking(), &helper)
            .await
            .unwrap();

        let ended = ran.session.unwrap();
        assert_eq!((ended.started, ended.ended), (at(13, 0), Some(at(14, 0)))); // one clock call
        assert_eq!(core.sessions().running().await.unwrap(), None);
        assert_eq!(started.end, None);
    }

    #[test]
    fn quiet_as_text() {
        let ran = Ran {
            session: None,
            code: 0,
        };

        assert_eq!(ran.to_string(), "");
    }
}
