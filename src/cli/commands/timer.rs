//! `udo start [NODE]`, `udo stop`, `udo status [--short]`.

use std::{fmt, path::Path};

use serde::Serialize;

use crate::cli::{report::Report, resolve::resolve};
use crate::{
    Res,
    core::{Core, IsDone},
    model::{
        sessions::{Session, SessionStore},
        time::{Minutes, Time},
    },
};

#[derive(clap::Args)]
pub struct StartArgs {
    /// The task; default: the task of the current folder
    pub node: Option<String>,
}

#[derive(clap::Args)]
pub struct StatusArgs {
    /// Only "name time", nothing if idle (for status bars)
    #[arg(long)]
    pub short: bool,
}

/// A session as the timer commands show it.
#[derive(Serialize)]
pub struct SessionLine {
    pub task: String,
    pub started: Time,
    pub ended: Option<Time>,
    /// Whole minutes so far (running) or in total (stopped).
    pub minutes: u32,
    /// `manual` or the program (`tmux`, `idea`, …).
    pub source: String,
    /// Who may stop it (`manual`, `tmux:udo-…`).
    pub owner: String,
}

impl SessionLine {
    /// `session` timed up to `now` (a stopped one up to its end).
    pub(super) fn of(session: &Session, now: Time) -> Self {
        Self {
            task: session.task.name.clone(),
            started: session.start,
            ended: session.end,
            minutes: session.duration(now).get(),
            source: session.source.to_string(),
            owner: session.owner.to_string(),
        }
    }

    /// `lab 3 (1h12)`; a program's session: `lab 3 (1h12, tmux)`
    pub(super) fn text(&self) -> String {
        match self.source.as_str() {
            "manual" => format!("{} ({})", self.task, Minutes::new(self.minutes)),
            program => format!("{} ({}, {program})", self.task, Minutes::new(self.minutes)),
        }
    }
}

/// `start`'s result: the session now running.
#[derive(Serialize)]
pub struct Started(pub SessionLine);

/// `stop`'s result: the session that was stopped, if any.
#[derive(Serialize)]
pub struct Stopped(pub Option<SessionLine>);

/// `status`'s result.
#[derive(Serialize)]
pub struct Status {
    pub running: Option<SessionLine>,
    /// `--short`: changes the text only, the JSON is the same.
    #[serde(skip)]
    pub short: bool,
}

/// Start timing `args.node` at `now` (a running session is stopped first).
pub async fn start(core: &mut Core, cwd: &Path, now: Time, args: &StartArgs) -> Res<Started> {
    let path = resolve(core.tree(), args.node.as_deref(), cwd)?;
    let session = core
        .start(&path, now)
        .await
        .map_err(|e| match e.downcast_ref::<IsDone>() {
            Some(done) => format!("{done}: use udo done --undo").into(),
            None => e,
        })?;
    Ok(Started(SessionLine::of(&session, now)))
}

/// Stop the running session at `now`.
pub async fn stop(core: &Core, now: Time) -> Res<Stopped> {
    let stopped = core.stop(now).await?;
    Ok(Stopped(stopped.as_ref().map(|s| SessionLine::of(s, now))))
}

/// What runs right now, timed up to `now`.
pub async fn status(core: &Core, now: Time, args: &StatusArgs) -> Res<Status> {
    let running = core.sessions().running().await?;
    Ok(Status {
        running: running.as_ref().map(|s| SessionLine::of(s, now)),
        short: args.short,
    })
}

impl fmt::Display for Started {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "running: {}", self.0.text())
    }
}

impl fmt::Display for Stopped {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.0 {
            Some(line) => write!(f, "stopped: {}", line.text()),
            None => write!(f, "no session running"),
        }
    }
}

impl fmt::Display for Status {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match (&self.running, self.short) {
            (Some(line), true) => write!(f, "{} {}", line.task, Minutes::new(line.minutes)),
            (Some(line), false) => write!(f, "running: {}", line.text()),
            (None, true) => Ok(()),
            (None, false) => write!(f, "no session running"),
        }
    }
}

impl Report for Started {}
impl Report for Stopped {}
impl Report for Status {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        cli::report::render,
        test_util::{at, core}, // core: disk_tree, root: [a, ws: [b]]
    };

    fn named(node: &str) -> StartArgs {
        StartArgs {
            node: Some(node.into()),
        }
    }

    #[tokio::test]
    async fn start_by_name_and_status_counts_up() {
        let (tmp, mut core) = core().await;

        let started = start(&mut core, tmp.path(), at(14, 0), &named("b"))
            .await
            .unwrap();
        let later = status(&core, at(15, 12), &StatusArgs { short: false })
            .await
            .unwrap();

        assert_eq!(started.to_string(), "running: b (0m)");
        assert_eq!(later.to_string(), "running: b (1h12)");
    }

    #[tokio::test]
    async fn a_done_task_says_how_to_reopen_it() {
        let (tmp, mut core) = core().await;
        core.set_done(&[1, 0], true, at(13, 0)).await.unwrap();

        let result = start(&mut core, tmp.path(), at(14, 0), &named("b")).await;

        let err = result.err().expect("started a done task");
        assert_eq!(err.to_string(), "b is done: use udo done --undo");
    }

    /// No argument: the node of the current folder ("ws" here, which is a
    /// container, so the start is refused).
    #[tokio::test]
    async fn start_without_argument_takes_the_current_folder() {
        let (tmp, mut core) = core().await;

        let err = start(
            &mut core,
            &tmp.path().join("ws"),
            at(14, 0),
            &StartArgs { node: None },
        )
        .await
        .err()
        .unwrap();

        assert!(err.to_string().contains("not a task"), "{err}");
    }

    #[tokio::test]
    async fn stop_reports_the_length() {
        let (tmp, mut core) = core().await;
        start(&mut core, tmp.path(), at(14, 0), &named("a"))
            .await
            .unwrap();

        let stopped = stop(&core, at(14, 45)).await.unwrap();

        assert_eq!(stopped.to_string(), "stopped: a (45m)");
    }

    #[tokio::test]
    async fn nothing_running() {
        let (_tmp, core) = core().await;

        let stopped = stop(&core, at(14, 0)).await.unwrap();
        let long = status(&core, at(14, 0), &StatusArgs { short: false })
            .await
            .unwrap();
        let short = status(&core, at(14, 0), &StatusArgs { short: true })
            .await
            .unwrap();

        assert_eq!(stopped.to_string(), "no session running");
        assert_eq!(long.to_string(), "no session running");
        assert_eq!(short.to_string(), ""); // `emit` prints nothing
    }

    /// A program's session names the program; the JSON has source and owner.
    #[tokio::test]
    async fn status_of_a_program_session_shows_its_source() {
        let (_tmp, mut core) = core().await;
        let (source, owner) = ("tmux".parse().unwrap(), "tmux:a".parse().unwrap());
        core.track_start(&[0], source, owner, at(14, 0))
            .await
            .unwrap();

        let running = status(&core, at(15, 12), &StatusArgs { short: false })
            .await
            .unwrap();

        assert_eq!(running.to_string(), "running: a (1h12, tmux)");
        let json: serde_json::Value =
            serde_json::from_str(&render(&running, true).unwrap()).unwrap();
        assert_eq!(json["running"]["source"], "tmux");
        assert_eq!(json["running"]["owner"], "tmux:a");
    }

    #[tokio::test]
    async fn short_status_is_name_and_time() {
        let (tmp, mut core) = core().await;
        start(&mut core, tmp.path(), at(14, 0), &named("a"))
            .await
            .unwrap();

        let short = status(&core, at(15, 12), &StatusArgs { short: true })
            .await
            .unwrap();

        assert_eq!(short.to_string(), "a 1h12");
    }

    #[tokio::test]
    async fn json_has_task_start_and_minutes() {
        let (tmp, mut core) = core().await;
        start(&mut core, tmp.path(), at(14, 0), &named("a"))
            .await
            .unwrap();
        let running = status(&core, at(15, 12), &StatusArgs { short: true })
            .await
            .unwrap();

        let json: serde_json::Value =
            serde_json::from_str(&render(&running, true).unwrap()).unwrap();

        assert_eq!(json["running"]["task"], "a");
        assert_eq!(json["running"]["minutes"], 72);
        assert!(json.get("short").is_none());
    }
}
