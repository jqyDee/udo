//! `udo done [NODE]` and `udo done --undo [NODE]`: mark a task done or
//! reopen it. Marking the timed task done stops the timer (a `Core` rule).

use std::{fmt, path::Path};

use serde::Serialize;

use super::timer::SessionLine;
use crate::cli::{
    report::Report,
    resolve::{path_text, resolve},
};
use crate::{Res, core::Core, model::time::Time};

#[derive(clap::Args)]
pub struct DoneArgs {
    /// The task; default: the task of the current folder
    pub node: Option<String>,
    /// Reopen the task instead
    #[arg(long)]
    pub undo: bool,
}

/// A task's status after `done`, and the timer it stopped.
#[derive(Serialize)]
pub struct Marked {
    pub path: String,
    pub status: String,
    pub stopped: Option<SessionLine>,
}

/// Mark the task done at `now`, or reopen it (`--undo`).
pub async fn run(core: &mut Core, cwd: &Path, now: Time, args: &DoneArgs) -> Res<Marked> {
    let path = resolve(core.tree(), args.node.as_deref(), cwd)?;
    let stopped = core.set_done(&path, !args.undo, now).await?;
    let with_sessions = core.tasks_with_sessions().await?;
    let node = core.tree().get(&path).ok_or("no such node")?;
    let task = node.as_task().ok_or("only tasks can be done")?;
    Ok(Marked {
        path: path_text(core.tree(), &path),
        status: task.status(with_sessions.contains(&node.id())).to_string(),
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
    use clap::Parser;

    use super::*;
    use crate::{
        cli::Cli,
        model::sessions::SessionStore,
        test_util::{at, core}, // core: disk_tree, root: [a, ws: [b]]
    };

    fn args(node: &str, undo: bool) -> DoneArgs {
        DoneArgs {
            node: Some(node.into()),
            undo,
        }
    }

    #[tokio::test]
    async fn done_marks_the_task_done() {
        let (tmp, mut core) = core().await;

        let marked = run(&mut core, tmp.path(), at(14, 0), &args("b", false))
            .await
            .unwrap();

        assert_eq!(marked.to_string(), "ws/b -> done");
    }

    #[tokio::test]
    async fn undo_reopens_to_do_or_started() {
        let (tmp, mut core) = core().await;
        core.add_session(&[0], at(9, 0), at(10, 0)).await.unwrap(); // "a" has time
        for node in ["a", "b"] {
            run(&mut core, tmp.path(), at(14, 0), &args(node, false))
                .await
                .unwrap();
        }

        let a = run(&mut core, tmp.path(), at(15, 0), &args("a", true)).await;
        let b = run(&mut core, tmp.path(), at(15, 0), &args("b", true)).await;

        assert_eq!(a.unwrap().to_string(), "a -> started");
        assert_eq!(b.unwrap().to_string(), "ws/b -> to do");
    }

    #[tokio::test]
    async fn done_on_the_timed_task_stops_the_timer() {
        let (tmp, mut core) = core().await;
        core.start(&[0], at(14, 0)).await.unwrap();

        let marked = run(&mut core, tmp.path(), at(14, 45), &args("a", false))
            .await
            .unwrap();

        assert_eq!(marked.to_string(), "a -> done\nstopped: a (45m)");
        assert_eq!(core.sessions().running().await.unwrap(), None);
    }

    #[tokio::test]
    async fn a_container_cannot_be_done() {
        let (tmp, mut core) = core().await;

        let result = run(&mut core, tmp.path(), at(14, 0), &args("ws", false)).await;

        assert!(result.is_err());
    }

    #[test]
    fn done_parses_and_mark_does_not() {
        assert!(Cli::try_parse_from(["udo", "done"]).is_ok());
        assert!(Cli::try_parse_from(["udo", "done", "--undo", "lab 3"]).is_ok());
        assert!(Cli::try_parse_from(["udo", "mark", "done", "lab 3"]).is_err());
    }
}
