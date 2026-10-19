//! `udo show [NODE]`: one node in detail, with the time tracked on it (a
//! container: on every task below it).

use std::{
    fmt,
    path::{Path, PathBuf},
};

use chrono::Local;
use serde::Serialize;

use crate::cli::{
    report::Report,
    resolve::{path_text, resolve},
};
use crate::{
    DATE_FMT, Res,
    core::Core,
    estimate::{Shown as Estimated, Totals, text},
    model::{
        id::NodeId,
        node::NodeBody,
        sessions::TimeSummary,
        time::{Left, Minutes, Time},
    },
};

#[derive(clap::Args)]
pub struct ShowArgs {
    /// Default: the node of the current folder
    pub node: Option<String>,
}

/// One node in detail. Containers have `kind`, tasks `status`, `due` and
/// `overdue`. Estimate vs. actual (#9): tasks have `estimate`, `own`,
/// `learned` and `diff`, containers the totals of their tasks.
#[derive(Serialize)]
pub struct Shown {
    /// The plain ID; as NODE: `id:<this>` (survives renames).
    pub id: NodeId,
    pub path: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub due: Option<Time>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub overdue: Option<bool>,
    pub dir: Option<PathBuf>,
    pub description: Option<String>,
    /// Whole minutes over all sessions, a running one up to now.
    pub minutes: u32,
    pub sessions: usize,
    /// The timer runs on this task (or on one below this container).
    pub running: bool,
    /// A task's estimate; its keys sit at the top level of the JSON.
    #[serde(flatten)]
    pub task_estimate: Option<TaskEstimate>,
    /// A container's totals; its keys sit at the top level of the JSON.
    #[serde(flatten)]
    pub container_estimate: Option<ContainerEstimate>,
}

/// A task's estimate next to its tracked time (#9). JSON: minutes, `null`
/// where there is none.
#[derive(Serialize)]
pub struct TaskEstimate {
    /// What udo goes by: yours, else the learned one.
    pub estimate: Option<u32>,
    pub own: Option<u32>,
    pub learned: Option<u32>,
    /// Tracked minus `estimate`, signed: positive is over.
    pub diff: Option<i64>,
    /// The `estimate:` line; made in `run` (it names containers: needs the
    /// tree, `Display` has none).
    #[serde(skip)]
    pub line: String,
    /// Done: a `result:` line instead of `left:`.
    #[serde(skip)]
    pub done: bool,
}

impl TaskEstimate {
    fn of(shown: &Estimated, line: String, done: bool, tracked: Minutes) -> Self {
        let estimate = shown.minutes().map(Minutes::get);
        Self {
            estimate,
            own: shown.own.map(Minutes::get),
            learned: shown.learned.map(|e| e.minutes.get()),
            diff: estimate.map(|e| i64::from(tracked.get()) - i64::from(e)),
            line,
            done,
        }
    }
}

/// A container's sums over the tasks below it (`estimate::Totals`), in
/// minutes for the JSON.
#[derive(Serialize)]
pub struct ContainerEstimate {
    pub estimated: u32,
    pub estimated_tasks: usize,
    pub tasks: usize,
    pub left: u32,
    pub over: usize,
    pub done_estimated: u32,
    pub done_actual: u32,
    /// For the text lines (`estimate::text`).
    #[serde(skip)]
    pub totals: Totals,
}

impl From<Totals> for ContainerEstimate {
    fn from(t: Totals) -> Self {
        Self {
            estimated: t.estimated.get(),
            estimated_tasks: t.estimated_tasks,
            tasks: t.tasks,
            left: t.left.get(),
            over: t.over,
            done_estimated: t.done_estimated.get(),
            done_actual: t.done_actual.get(),
            totals: t,
        }
    }
}

/// The details of `args.node`, sessions timed up to `now`.
pub async fn run(core: &Core, cwd: &Path, now: Time, args: &ShowArgs) -> Res<Shown> {
    let tree = core.tree();
    let path = resolve(tree, args.node.as_deref(), cwd)?;
    let node = tree.get(&path).ok_or("no such node")?;

    let sessions = core.sessions_of(&path).await?;
    let summary = TimeSummary::of(&sessions, now);

    let (kind, status, due, overdue) = match &node.body {
        NodeBody::Container(c) => (Some(c.kind.to_string()), None, None, None),
        NodeBody::Task(t) => {
            let with_sessions = core.tasks_with_sessions().await?;
            let status = t.status(with_sessions.contains(&node.id()));
            (
                None,
                Some(status.to_string()),
                Some(t.due_date),
                Some(t.is_overdue(now)),
            )
        }
    };
    let (task_estimate, container_estimate) = match &node.body {
        NodeBody::Task(t) => {
            let shown = core.estimate(&path, now).await?;
            let line = text::estimate_text(tree, &shown);
            let e = TaskEstimate::of(&shown, line, t.done_at.is_some(), summary.duration);
            (Some(e), None)
        }
        NodeBody::Container(_) => (None, Some(core.totals(&path, now).await?.into())),
    };
    Ok(Shown {
        id: node.id(),
        path: path_text(tree, &path),
        name: node.name().into(),
        kind,
        status,
        due,
        overdue,
        dir: node.dir().map(Path::to_path_buf),
        description: node.header.description.clone(),
        minutes: summary.duration.get(),
        sessions: summary.sessions,
        running: summary.running,
        task_estimate,
        container_estimate,
    })
}

impl fmt::Display for Shown {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let line = |f: &mut fmt::Formatter<'_>, label: &str, value: &dyn fmt::Display| {
            write!(f, "\n{:<13}{value}", format!("{label}:"))
        };
        write!(f, "{}", self.name)?;
        line(f, "path", &self.path)?;
        line(f, "id", &self.id)?;
        if let Some(kind) = &self.kind {
            line(f, "kind", kind)?;
        }
        if let Some(status) = &self.status {
            line(f, "status", status)?;
        }
        if let Some(due) = &self.due {
            let overdue = if self.overdue == Some(true) {
                " (overdue)"
            } else {
                ""
            };
            let due = format!("{}{overdue}", due.with_timezone(&Local).format(DATE_FMT));
            line(f, "due", &due)?;
        }
        if let Some(dir) = &self.dir {
            line(f, "folder", &dir.display())?;
        }
        if let Some(description) = &self.description {
            line(f, "description", description)?;
        }

        // as the TUI details: estimate, tracked, then left / result / done
        if let Some(e) = &self.task_estimate {
            line(f, "estimate", &e.line)?;
        }
        if let Some(c) = &self.container_estimate {
            line(f, "estimated", &text::estimated_text(&c.totals))?;
        }
        let tracked = TimeSummary {
            duration: Minutes::new(self.minutes),
            sessions: self.sessions,
            running: self.running,
        };
        line(f, "tracked", &tracked)?;

        if let Some(e) = &self.task_estimate {
            let tracked = Minutes::new(self.minutes);
            match (e.estimate.map(Minutes::new), e.done) {
                (Some(est), true) => line(f, "result", &text::result_text(est, tracked))?,
                (Some(est), false) => line(f, "left", &text::left_text(Left::of(est, tracked)))?,
                (None, false) => line(f, "left", &"-")?,
                (None, true) => {} // done without an estimate: nothing to compare
            }
        }
        if let Some(c) = &self.container_estimate {
            if let Some(left) = text::totals_left_text(&c.totals) {
                line(f, "left", &left)?;
            }
            if let Some(done) = text::done_text(&c.totals) {
                line(f, "done", &done)?;
            }
        }
        Ok(())
    }
}

impl Report for Shown {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        cli::report::render,
        model::{
            node::{BodyPatch, NodePatch},
            task::TaskPatch,
        },
        test_util::{at, core, local}, // core: disk_tree, root: [a, ws: [b]]
    };

    fn named(node: &str) -> ShowArgs {
        ShowArgs {
            node: Some(node.into()),
        }
    }

    /// Set the own estimate of the task at `path` (through `Core::edit`).
    async fn set_own(core: &mut Core, path: &[usize], minutes: u32) {
        let patch = NodePatch {
            body: Some(BodyPatch::Task(TaskPatch {
                estimate: Some(Some(Minutes::new(minutes))),
                ..Default::default()
            })),
            ..Default::default()
        };
        core.edit(path, patch).await.unwrap();
    }

    /// `udo show NODE` at 12:00, as text and as JSON.
    async fn both(core: &Core, cwd: &Path, node: &str) -> (String, serde_json::Value) {
        let shown = run(core, cwd, at(12, 0), &named(node)).await.unwrap();
        let json = serde_json::from_str(&render(&shown, true).unwrap()).unwrap();
        (shown.to_string(), json)
    }

    #[tokio::test]
    async fn an_open_task_shows_its_estimate_and_what_is_left() {
        // b: yours 1h30, 20m tracked
        let (tmp, mut core) = core().await;
        set_own(&mut core, &[1, 0], 90).await;
        core.add_session(&[1, 0], at(9, 0), at(9, 20), at(12, 0))
            .await
            .unwrap();

        let (text, json) = both(&core, tmp.path(), "b").await;

        assert!(text.contains("\nestimate:    1h30 (yours)\n"), "{text}");
        assert!(text.ends_with("\nleft:        1h10"), "{text}");
        assert_eq!(json["estimate"], 90);
        assert_eq!(json["own"], 90);
        assert_eq!(json["diff"], -70);
        assert!(json["learned"].is_null()); // nothing to learn from
        assert!(json.get("estimated").is_none()); // a container's key
    }

    #[tokio::test]
    async fn a_task_without_an_estimate_has_nulls_and_no_left() {
        let (tmp, core) = core().await;

        let (text, json) = both(&core, tmp.path(), "b").await;

        assert!(text.contains("\nestimate:    -\n"), "{text}");
        assert!(text.ends_with("\nleft:        -"), "{text}");
        for key in ["estimate", "own", "learned", "diff"] {
            assert!(json[key].is_null(), "{key}: {json}");
        }
    }

    #[tokio::test]
    async fn a_done_task_shows_its_result() {
        // b: yours 1h, done after 1h20
        let (tmp, mut core) = core().await;
        set_own(&mut core, &[1, 0], 60).await;
        core.add_session(&[1, 0], at(9, 0), at(10, 20), at(12, 0))
            .await
            .unwrap();
        core.set_done(&[1, 0], true, at(11, 0)).await.unwrap();

        let (text, json) = both(&core, tmp.path(), "b").await;

        assert!(text.ends_with("\nresult:      over by 20m"), "{text}");
        assert!(!text.contains("\nleft:"), "{text}");
        assert_eq!(json["diff"], 20);
    }

    #[tokio::test]
    async fn a_container_shows_the_totals() {
        // ws: [b (yours 1h, 20m tracked)]
        let (tmp, mut core) = core().await;
        set_own(&mut core, &[1, 0], 60).await;
        core.add_session(&[1, 0], at(9, 0), at(9, 20), at(12, 0))
            .await
            .unwrap();

        let (text, json) = both(&core, tmp.path(), "ws").await;

        assert!(
            text.contains("\nestimated:   1h (1 of 1 tasks)\n"),
            "{text}"
        );
        assert!(text.ends_with("\nleft:        40m"), "{text}"); // no done row
        assert_eq!(json["estimated"], 60);
        assert_eq!(json["estimated_tasks"], 1);
        assert_eq!(json["tasks"], 1);
        assert_eq!(json["left"], 40);
        assert_eq!(json["over"], 0);
        assert_eq!(json["done_estimated"], 0);
        assert!(json.get("own").is_none()); // a task's key
    }

    #[tokio::test]
    async fn a_container_with_done_tasks_compares_them() {
        // ws: [b (yours 1h, done after 1h20)]
        let (tmp, mut core) = core().await;
        set_own(&mut core, &[1, 0], 60).await;
        core.add_session(&[1, 0], at(9, 0), at(10, 20), at(12, 0))
            .await
            .unwrap();
        core.set_done(&[1, 0], true, at(11, 0)).await.unwrap();

        let (text, json) = both(&core, tmp.path(), "ws").await;

        assert!(
            text.ends_with(
                "\nleft:        0m\ndone:        1h estimated, 1h20 tracked (over by 20m)"
            ),
            "{text}"
        );
        assert_eq!(json["done_estimated"], 60);
        assert_eq!(json["done_actual"], 80);
    }

    #[tokio::test]
    async fn a_container_without_estimates_has_no_left_or_done() {
        let (tmp, core) = core().await;

        let (text, _) = both(&core, tmp.path(), "ws").await;

        assert!(
            text.ends_with("\nestimated:   - (0 of 1 tasks)\ntracked:     0m in 0 sessions"),
            "{text}"
        );
    }

    #[tokio::test]
    async fn a_task_with_its_sessions() {
        let (tmp, mut core) = core().await;
        core.start(&[1, 0], at(14, 0)).await.unwrap();
        core.stop(at(14, 30)).await.unwrap();
        core.start(&[1, 0], at(15, 0)).await.unwrap();

        let shown = run(&core, tmp.path(), at(15, 42), &named("b"))
            .await
            .unwrap();

        assert_eq!(shown.path, "ws/b");
        assert_eq!(shown.status.as_deref(), Some("started"));
        assert_eq!(
            (shown.minutes, shown.sessions, shown.running),
            (72, 2, true)
        );
    }

    #[tokio::test]
    async fn a_container_sums_the_tasks_below_it() {
        let (tmp, mut core) = core().await;
        core.start(&[1, 0], at(14, 0)).await.unwrap();
        core.start(&[0], at(14, 30)).await.unwrap(); // "a" is not below "ws"
        core.stop(at(15, 0)).await.unwrap();

        let ws = run(&core, tmp.path(), at(15, 0), &named("ws"))
            .await
            .unwrap();
        let root = run(&core, tmp.path(), at(15, 0), &named("/"))
            .await
            .unwrap();

        assert_eq!(
            (ws.kind.as_deref(), ws.minutes, ws.sessions),
            (Some("workspace"), 30, 1)
        );
        assert!(!ws.running);
        assert_eq!((root.minutes, root.sessions), (60, 2));
    }

    /// Past due and not done: overdue, in the JSON too. Done: not overdue.
    #[tokio::test]
    async fn a_task_past_due_is_overdue_until_done() {
        let (tmp, mut core) = core().await;
        let due = core.tree().get(&[0]).unwrap().as_task().unwrap().due_date;
        let later = due + chrono::TimeDelta::minutes(1);

        let shown = run(&core, tmp.path(), later, &named("a")).await.unwrap();
        assert_eq!(
            (shown.status.as_deref(), shown.overdue),
            (Some("to do"), Some(true))
        );
        let json: serde_json::Value = serde_json::from_str(&render(&shown, true).unwrap()).unwrap();
        assert_eq!(json["overdue"], true);

        core.set_done(&[0], true, later).await.unwrap();
        let shown = run(&core, tmp.path(), later, &named("a")).await.unwrap();
        assert_eq!(
            (shown.status.as_deref(), shown.overdue),
            (Some("done"), Some(false))
        );
    }

    #[tokio::test]
    async fn a_container_has_no_overdue() {
        let (tmp, core) = core().await;

        let shown = run(&core, tmp.path(), at(12, 0), &named("ws"))
            .await
            .unwrap();

        let json: serde_json::Value = serde_json::from_str(&render(&shown, true).unwrap()).unwrap();
        assert!(json.get("overdue").is_none());
    }

    /// The JSON's `id` is the plain ID; `id:<it>` names the same node again,
    /// for any command (hooks written by hand: `udo track start --task
    /// id:<it> …`).
    #[tokio::test]
    async fn the_id_resolves_back_to_the_node() {
        let (tmp, core) = core().await;

        let shown = run(&core, tmp.path(), at(12, 0), &named("b"))
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_str(&render(&shown, true).unwrap()).unwrap();
        let id = json["id"].as_str().unwrap();

        assert_eq!(id, core.tree().get(&[1, 0]).unwrap().id().to_string());
        let again = run(&core, tmp.path(), at(12, 0), &named(&format!("id:{id}")))
            .await
            .unwrap();
        assert_eq!(again.path, "ws/b");
    }

    #[test]
    fn text_has_one_labelled_line_per_field() {
        let shown = Shown {
            id: "01a10224-0f2b-77da-adc0-deaeebedad96".parse().unwrap(),
            path: "uni/lab 3".into(),
            name: "lab 3".into(),
            kind: None,
            status: Some("to do".into()),
            due: None,
            overdue: None,
            dir: Some(PathBuf::from("/uni/lab_3")),
            description: Some("sheet 3".into()),
            minutes: 72,
            sessions: 1,
            running: true,
            task_estimate: None,
            container_estimate: None,
        };

        assert_eq!(
            shown.to_string(),
            "lab 3\n\
             path:        uni/lab 3\n\
             id:          01a10224-0f2b-77da-adc0-deaeebedad96\n\
             status:      to do\n\
             folder:      /uni/lab_3\n\
             description: sheet 3\n\
             tracked:     1h12 in 1 session, running"
        );
    }

    #[test]
    fn an_overdue_due_line_says_so() {
        let shown = Shown {
            id: NodeId::new(),
            path: "lab 3".into(),
            name: "lab 3".into(),
            kind: None,
            status: Some("started".into()),
            due: Some(local(22, 0)),
            overdue: Some(true),
            dir: None,
            description: None,
            minutes: 0,
            sessions: 0,
            running: false,
            task_estimate: None,
            container_estimate: None,
        };

        assert!(
            shown
                .to_string()
                .contains("\ndue:         2026-10-15 22:00 (overdue)\n"),
            "{shown}"
        );
    }
}
