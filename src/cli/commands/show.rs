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
        sessions::{SessionQuery, SessionStore},
        time::{Minutes, Time},
    },
};

#[derive(clap::Args)]
pub struct ShowArgs {
    /// Default: the node of the current folder
    pub node: Option<String>,
}

/// One node in detail. Containers have `kind`, tasks `status` + `due`.
#[derive(Serialize)]
pub struct Shown {
    pub path: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub due: Option<Time>,
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

    // the task itself, or every task below the container
    let tasks: Vec<NodeId> = match &node.body {
        NodeBody::Task(_) => vec![node.id()],
        NodeBody::Container(_) => tree
            .rows()
            .into_iter()
            .filter(|r| r.path.starts_with(&path) && r.node.as_task().is_some())
            .map(|r| r.node.id())
            .collect(),
    };
    let query = SessionQuery {
        tasks: Some(tasks.clone()),
        ..Default::default()
    };
    let sessions = core.sessions().query(&query).await?;
    let minutes = sessions.iter().map(|s| s.duration(now).get()).sum();
    let running = sessions.iter().any(|s| s.end.is_none());

    let (kind, status, due) = match &node.body {
        NodeBody::Container(c) => (Some(c.kind.to_string()), None, None),
        NodeBody::Task(t) => (None, Some(t.status.to_string()), Some(t.due_date)),
    };
    Ok(Shown {
        path: path_text(tree, &path),
        name: node.name().into(),
        kind,
        status,
        due,
        dir: node.dir().map(Path::to_path_buf),
        description: node.header.description.clone(),
        minutes,
        sessions: sessions.len(),
        running,
    })
}

impl fmt::Display for Shown {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let line = |f: &mut fmt::Formatter<'_>, label: &str, value: &dyn fmt::Display| {
            write!(f, "\n{:<13}{value}", format!("{label}:"))
        };
        write!(f, "{}", self.name)?;
        line(f, "path", &self.path)?;
        if let Some(kind) = &self.kind {
            line(f, "kind", kind)?;
        }
        if let Some(status) = &self.status {
            line(f, "status", status)?;
        }
        if let Some(due) = &self.due {
            line(f, "due", &due.with_timezone(&Local).format(DATE_FMT))?;
        }
        if let Some(dir) = &self.dir {
            line(f, "folder", &dir.display())?;
        }
        if let Some(description) = &self.description {
            line(f, "description", description)?;
        }

        let sessions = if self.sessions == 1 {
            "session"
        } else {
            "sessions"
        };
        let running = if self.running { ", running" } else { "" };
        let tracked =
            format!("{} in {} {sessions}{running}", Minutes::new(self.minutes), self.sessions);
        line(f, "tracked", &tracked)
    }
}

impl Report for Shown {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::{at, core}; // core: disk_tree, root: [a, ws: [b]]

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
        assert_eq!(shown.status.as_deref(), Some("in progress"));
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

    #[test]
    fn text_has_one_labelled_line_per_field() {
        let shown = Shown {
            path: "uni/lab 3".into(),
            name: "lab 3".into(),
            kind: None,
            status: Some("to do".into()),
            due: None,
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
             status:      to do\n\
             folder:      /uni/lab_3\n\
             description: sheet 3\n\
             tracked:     1h12 in 1 session, running"
        );
    }
}
