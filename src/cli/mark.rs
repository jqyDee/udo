//! `udo done [NODE]` and `udo mark STATUS [NODE]`: set a task's status.
//! Marking the timed task done stops the timer (a `Core` rule).

use std::{fmt, path::Path};

use serde::Serialize;

use super::{
    parse::StatusArg,
    report::Report,
    resolve::{path_text, resolve},
    timer::SessionLine,
};
use crate::{
    Res,
    core::Core,
    model::{task::TaskStatus, time::Time},
};

#[derive(clap::Args)]
pub struct DoneArgs {
    /// The task; default: the task of the current folder
    pub node: Option<String>,
}

#[derive(clap::Args)]
pub struct MarkArgs {
    pub status: StatusArg,
    /// The task; default: the task of the current folder
    pub node: Option<String>,
}

/// A task's new status, and the timer it stopped.
#[derive(Serialize)]
pub struct Marked {
    pub path: String,
    pub status: String,
    pub stopped: Option<SessionLine>,
}

/// `done`: mark the task finished at `now`.
pub async fn done(core: &mut Core, cwd: &Path, now: Time, args: &DoneArgs) -> Res<Marked> {
    set(core, cwd, now, args.node.as_deref(), TaskStatus::Finished).await
}

/// `mark`: give the task `args.status` at `now`.
pub async fn mark(core: &mut Core, cwd: &Path, now: Time, args: &MarkArgs) -> Res<Marked> {
    set(core, cwd, now, args.node.as_deref(), args.status.into()).await
}

async fn set(
    core: &mut Core,
    cwd: &Path,
    now: Time,
    node: Option<&str>,
    status: TaskStatus,
) -> Res<Marked> {
    let path = resolve(core.tree(), node, cwd)?;
    let stopped = core.set_status(&path, status, now).await?;
    Ok(Marked {
        path: path_text(core.tree(), &path),
        status: status.to_string(),
        stopped: stopped.as_ref().map(|s| SessionLine::of(s, now)),
    })
}

impl fmt::Display for Marked {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} -> {}", self.path, self.status)?;
        if let Some(stopped) = &self.stopped {
            write!(f, "\nstopped: {}", stopped.text())?;
        }
        Ok(())
    }
}

impl Report for Marked {}

#[cfg(test)]
mod tests {
    use chrono::{FixedOffset, TimeZone};

    use super::*;
    use crate::{model::sessions::SessionStore, storage::Storage, test_util::disk_tree};

    fn at(h: u32, m: u32) -> Time {
        FixedOffset::east_opt(2 * 3600)
            .unwrap()
            .with_ymd_and_hms(2026, 10, 15, h, m, 0)
            .unwrap()
    }

    async fn core() -> (tempfile::TempDir, Core) {
        let (tmp, tree) = disk_tree().await; // root: [a, ws: [b]]
        (tmp, Core::new(tree, Storage::in_memory()))
    }

    #[tokio::test]
    async fn mark_sets_the_status() {
        let (tmp, mut core) = core().await;
        let args = MarkArgs {
            status: StatusArg::InProgress,
            node: Some("b".into()),
        };

        let marked = mark(&mut core, tmp.path(), at(14, 0), &args).await.unwrap();

        assert_eq!(marked.to_string(), "ws/b -> in progress");
    }

    #[tokio::test]
    async fn done_on_the_timed_task_stops_the_timer() {
        let (tmp, mut core) = core().await;
        core.start(&[0], at(14, 0)).await.unwrap();

        let marked = done(
            &mut core,
            tmp.path(),
            at(14, 45),
            &DoneArgs {
                node: Some("a".into()),
            },
        )
        .await
        .unwrap();

        assert_eq!(marked.to_string(), "a -> done\nstopped: a (45m)");
        assert_eq!(core.sessions().running().await.unwrap(), None);
    }

    #[tokio::test]
    async fn a_container_has_no_status() {
        let (tmp, mut core) = core().await;

        let result = done(
            &mut core,
            tmp.path(),
            at(14, 0),
            &DoneArgs {
                node: Some("ws".into()),
            },
        )
        .await;

        assert!(result.is_err());
    }
}
