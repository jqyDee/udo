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
    model::{
        id::NodeId,
        node::NodeBody,
        sessions::TimeSummary,
        time::{Minutes, Time},
    },
};

#[derive(clap::Args)]
pub struct ShowArgs {
    /// Default: the node of the current folder
    pub node: Option<String>,
}

/// One node in detail. Containers have `kind`, tasks `status`, `due` and
/// `overdue`.
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
            (None, Some(status.to_string()), Some(t.due_date), Some(t.is_overdue(now)))
        }
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

        let tracked = TimeSummary {
            duration: Minutes::new(self.minutes),
            sessions: self.sessions,
            running: self.running,
        };
        line(f, "tracked", &tracked)
    }
}

impl Report for Shown {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        cli::report::render,
        test_util::{at, core, local}, // core: disk_tree, root: [a, ws: [b]]
    };

    fn named(node: &str) -> ShowArgs {
        ShowArgs {
            node: Some(node.into()),
        }
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
        assert_eq!((shown.minutes, shown.sessions, shown.running), (72, 2, true));
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

        assert_eq!((ws.kind.as_deref(), ws.minutes, ws.sessions), (Some("workspace"), 30, 1));
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
        assert_eq!((shown.status.as_deref(), shown.overdue), (Some("to do"), Some(true)));
        let json: serde_json::Value = serde_json::from_str(&render(&shown, true).unwrap()).unwrap();
        assert_eq!(json["overdue"], true);

        core.set_done(&[0], true, later).await.unwrap();
        let shown = run(&core, tmp.path(), later, &named("a")).await.unwrap();
        assert_eq!((shown.status.as_deref(), shown.overdue), (Some("done"), Some(false)));
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

        let shown = run(&core, tmp.path(), at(12, 0), &named("b")).await.unwrap();
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
        };

        assert!(
            shown
                .to_string()
                .contains("\ndue:         2026-10-15 22:00 (overdue)\n"),
            "{shown}"
        );
    }
}
