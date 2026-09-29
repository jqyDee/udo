//! `udo ls [NODE]`: the tree from a node down. No argument: the node of the
//! current folder, or the root outside udo folders. A container lists
//! everything below it; a task lists only itself.

use std::{fmt, path::Path};

use chrono::Local;
use serde::Serialize;

use crate::cli::{
    report::Report,
    resolve::{path_text, resolve},
};
use crate::{
    DATE_FMT, Res,
    core::Core,
    model::{node::NodeBody, time::Time},
};

#[derive(clap::Args)]
pub struct LsArgs {
    /// Where to start; default: the node of the current folder, else the root
    pub node: Option<String>,
}

/// Everything below a node, one row per node, in tree order.
#[derive(Serialize)]
pub struct Listing {
    pub rows: Vec<ListRow>,
}

/// One node of a `Listing`. Containers have `kind`, tasks `status`, `due`
/// and `overdue`.
#[derive(Serialize)]
pub struct ListRow {
    pub path: String,
    pub name: String,
    /// Indentation; the listed node's children are 0.
    pub depth: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub due: Option<Time>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub overdue: Option<bool>,
}

/// The listing of `args.node` (see the module doc for the rules), overdue
/// as of `now`.
pub async fn run(core: &Core, cwd: &Path, now: Time, args: &LsArgs) -> Res<Listing> {
    let with_sessions = core.tasks_with_sessions().await?;
    let tree = core.tree();
    let start = match &args.node {
        Some(node) => resolve(tree, Some(node), cwd)?,
        None => resolve(tree, None, cwd).unwrap_or_default(), // outside udo: the root
    };
    let start_is_task = tree.get(&start).and_then(|n| n.as_task()).is_some();
    let rows = tree
        .rows()
        .into_iter()
        .filter(|r| {
            if start_is_task {
                r.path == start
            } else {
                r.path.starts_with(&start) && r.path != start
            }
        })
        .map(|r| {
            let path = path_text(tree, &r.path);
            let name = r.node.name();
            // children of the listed container are 0; a task listed alone too
            let depth = if start_is_task {
                0
            } else {
                r.depth - start.len()
            };
            match &r.node.body {
                NodeBody::Container(c) => {
                    ListRow::container(&path, name, depth, &c.kind.to_string())
                }
                NodeBody::Task(t) => {
                    let status = t.status(with_sessions.contains(&r.node.id()));
                    let (status, overdue) = (status.to_string(), t.is_overdue(now));
                    ListRow::task(&path, name, depth, &status, t.due_date, overdue)
                }
            }
        })
        .collect();
    Ok(Listing { rows })
}

impl ListRow {
    fn container(path: &str, name: &str, depth: usize, kind: &str) -> Self {
        Self {
            path: path.into(),
            name: name.into(),
            depth,
            kind: Some(kind.into()),
            status: None,
            due: None,
            overdue: None,
        }
    }

    fn task(path: &str, name: &str, depth: usize, status: &str, due: Time, overdue: bool) -> Self {
        Self {
            path: path.into(),
            name: name.into(),
            depth,
            kind: None,
            status: Some(status.into()),
            due: Some(due),
            overdue: Some(overdue),
        }
    }

    /// `name/` + kind for containers; name, status, local due and
    /// `overdue` for tasks.
    fn line(&self) -> String {
        let indent = "  ".repeat(self.depth);
        match (&self.kind, &self.status, &self.due) {
            (Some(kind), _, _) => format!("{:<30}{kind}", format!("{indent}{}/", self.name)),
            (_, Some(status), Some(due)) => format!(
                "{:<30}{status:<12}{}{}",
                format!("{indent}{}", self.name),
                due.with_timezone(&Local).format(DATE_FMT),
                if self.overdue == Some(true) {
                    "  overdue"
                } else {
                    ""
                }
            ),
            _ => format!("{indent}{}", self.name),
        }
    }
}

impl fmt::Display for Listing {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.rows.is_empty() {
            return write!(f, "(nothing here yet)");
        }
        let lines: Vec<String> = self.rows.iter().map(ListRow::line).collect();
        write!(f, "{}", lines.join("\n"))
    }
}

impl Report for Listing {}

#[cfg(test)]
mod tests {
    use chrono::{TimeDelta, TimeZone};

    use super::*;
    use crate::{
        cli::report::render,
        test_util::{at, core}, // core: disk_tree, root: [a, ws: [b]]
    };

    fn names(listing: &Listing) -> Vec<(usize, &str)> {
        listing
            .rows
            .iter()
            .map(|r| (r.depth, r.name.as_str()))
            .collect()
    }

    fn ls(node: Option<&str>) -> LsArgs {
        LsArgs {
            node: node.map(Into::into),
        }
    }

    async fn list(core: &Core, cwd: &Path, node: Option<&str>) -> Listing {
        run(core, cwd, at(12, 0), &ls(node)).await.unwrap()
    }

    #[tokio::test]
    async fn outside_udo_it_lists_everything() {
        let (_tmp, core) = core().await;
        let out = tempfile::tempdir().unwrap();

        let listing = list(&core, out.path(), None).await;

        assert_eq!(names(&listing), vec![(0, "a"), (0, "ws"), (1, "b")]);
    }

    #[tokio::test]
    async fn a_container_lists_what_is_below_it() {
        let (_tmp, core) = core().await;
        let out = tempfile::tempdir().unwrap();

        let listing = list(&core, out.path(), Some("ws")).await;

        assert_eq!(names(&listing), vec![(0, "b")]);
        assert_eq!(listing.rows[0].path, "ws/b");
    }

    #[tokio::test]
    async fn without_argument_it_starts_at_cwd() {
        let (tmp, core) = core().await;

        let listing = list(&core, &tmp.path().join("ws"), None).await;

        assert_eq!(names(&listing), vec![(0, "b")]);
    }

    #[tokio::test]
    async fn a_task_lists_itself() {
        let (_tmp, core) = core().await;
        let out = tempfile::tempdir().unwrap();

        let listing = list(&core, out.path(), Some("a")).await;

        assert_eq!(names(&listing), vec![(0, "a")]);
    }

    /// "a" has a session and is past due; "b" is done (so never overdue).
    #[tokio::test]
    async fn status_and_overdue_are_computed() {
        let (tmp, mut core) = core().await;
        core.add_session(&[0], at(9, 0), at(10, 0)).await.unwrap();
        core.set_done(&[1, 0], true, at(10, 0)).await.unwrap();
        let due = core.tree().get(&[0]).unwrap().as_task().unwrap().due_date;
        let now = due.max(at(10, 0)) + TimeDelta::minutes(1);

        let listing = run(&core, tmp.path(), now, &ls(Some("/"))).await.unwrap();

        let tasks: Vec<_> = listing
            .rows
            .iter()
            .filter(|r| r.kind.is_none())
            .map(|r| (r.name.as_str(), r.status.as_deref(), r.overdue))
            .collect();
        assert_eq!(
            tasks,
            vec![
                ("a", Some("started"), Some(true)),
                ("b", Some("done"), Some(false))
            ]
        );
        let json: serde_json::Value =
            serde_json::from_str(&render(&listing, true).unwrap()).unwrap();
        assert_eq!(json["rows"][0]["overdue"], true);
        assert!(json["rows"][1].get("overdue").is_none()); // "ws": a container
        assert_eq!(json["rows"][2]["status"], "done");
    }

    #[test]
    fn text_has_one_aligned_line_per_node() {
        let due = Local
            .with_ymd_and_hms(2026, 10, 15, 22, 0, 0)
            .unwrap()
            .fixed_offset();
        let listing = Listing {
            rows: vec![
                ListRow::container("uni", "uni", 0, "workspace"),
                ListRow::task("uni/lab 3", "lab 3", 1, "to do", due, false),
                ListRow::task("uni/lab 2", "lab 2", 1, "started", due, true),
            ],
        };

        let expected = format!(
            "{:<30}workspace\n{:<30}{:<12}2026-10-15 22:00\n{:<30}{:<12}2026-10-15 22:00  overdue",
            "uni/", "  lab 3", "to do", "  lab 2", "started"
        );
        assert_eq!(listing.to_string(), expected);
    }

    #[test]
    fn an_empty_listing_says_so() {
        assert_eq!(Listing { rows: vec![] }.to_string(), "(nothing here yet)");
    }
}
