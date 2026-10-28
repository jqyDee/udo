//! `udo edit [NODE] --name … --description … --due … --estimate … --kind …`:
//! change a node's own fields. Only what is given changes.

use std::{fmt, path::Path};

use chrono::NaiveDateTime;
use serde::Serialize;

use crate::cli::{
    parse::{self, Due, KindArg},
    report::Report,
    resolve::{path_text, resolve},
};
use crate::{
    Res,
    core::Core,
    model::{
        container::ContainerPatch,
        node::{BodyPatch, HeaderPatch, NodePatch},
        task::TaskPatch,
        time::{Minutes, local_to_fixed},
    },
    naming::normalize_name,
};

#[derive(clap::Args)]
pub struct EditArgs {
    /// Default: the node of the current folder
    pub node: Option<String>,
    /// New name (the folder keeps its name)
    #[arg(long)]
    pub name: Option<String>,
    /// New description; "" removes it
    #[arg(long)]
    pub description: Option<String>,
    /// Tasks: a rule like "fri 22:00" or "+7d 23:59", or "YYYY-MM-DD HH:MM"
    #[arg(long, value_parser = parse::due)]
    pub due: Option<Due>,
    /// Tasks: your own estimate, like 1h30, 2h or 45m; wins over the
    /// learned one. "" removes it
    #[arg(long)]
    pub estimate: Option<String>,
    /// Workspaces and projects: what the container is
    #[arg(long, value_enum)]
    pub kind: Option<KindArg>,
}

/// What `edit` saved.
#[derive(Serialize)]
pub struct Edited {
    /// The path after the change (a new name changes it).
    pub path: String,
}

/// Apply the given fields to `args.node`; `now` (local) is where relative
/// due rules start.
pub async fn run(core: &mut Core, cwd: &Path, now: NaiveDateTime, args: &EditArgs) -> Res<Edited> {
    if args.name.is_none()
        && args.description.is_none()
        && args.due.is_none()
        && args.estimate.is_none()
        && args.kind.is_none()
    {
        return Err(
            "nothing to change: give --name, --description, --due, --estimate or --kind".into(),
        );
    }
    let path = resolve(core.tree(), args.node.as_deref(), cwd)?;
    // say which flag does not fit, instead of the tree's "patch kind" error
    let is_task = core
        .tree()
        .get(&path)
        .is_some_and(|n| n.as_task().is_some());
    if args.due.is_some() && !is_task {
        return Err("--due is for tasks".into());
    }
    if args.estimate.is_some() && !is_task {
        return Err("--estimate is for tasks".into());
    }
    if args.kind.is_some() && is_task {
        return Err("--kind is for workspaces and projects".into());
    }
    let due = match args.due {
        Some(due) => {
            Some(local_to_fixed(due.after(now)).ok_or("that time doesn't exist (DST switch)")?)
        }
        None => None,
    };
    let estimate = args.estimate.as_deref().map(own_estimate).transpose()?;
    // the checks above: task flags only on tasks, `--kind` only on containers
    let body = if is_task {
        (due.is_some() || estimate.is_some()).then(|| {
            BodyPatch::Task(TaskPatch {
                due_date: due,
                estimate,
                ..Default::default()
            })
        })
    } else {
        args.kind.map(|kind| {
            BodyPatch::Container(ContainerPatch {
                kind: Some(kind.into()),
                ..Default::default()
            })
        })
    };
    let patch = NodePatch {
        header: HeaderPatch {
            name: args.name.as_deref().map(normalize_name),
            // "" -> Some(None): the tree removes a blank description
            description: args.description.clone().map(Some),
        },
        body,
    };
    core.edit(&path, patch).await?;
    Ok(Edited {
        path: path_text(core.tree(), &path),
    })
}

/// `--estimate`: blank removes it (`None`), else a duration like `1h30`.
fn own_estimate(text: &str) -> Result<Option<Minutes>, String> {
    if text.trim().is_empty() {
        return Ok(None);
    }
    text.parse().map(Some)
}

impl fmt::Display for Edited {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "edited {}", self.path)
    }
}

impl Report for Edited {}

#[cfg(test)]
mod tests {
    use chrono::Local;

    use super::*;
    use crate::{
        model::container::ContainerKind,
        test_util::{core, dt, thursday_noon}, // core: disk_tree, root: [a, ws: [b]]
    };

    fn edit(node: &str) -> EditArgs {
        EditArgs {
            node: Some(node.into()),
            name: None,
            description: None,
            due: None,
            estimate: None,
            kind: None,
        }
    }

    /// The own estimate of the task at `path`.
    fn estimate_of(core: &Core, path: &[usize]) -> Option<Minutes> {
        core.tree()
            .get(path)
            .and_then(|n| n.as_task())
            .unwrap()
            .estimate
    }

    #[tokio::test]
    async fn a_new_name_gives_the_new_path() {
        let (tmp, mut core) = core().await;
        let args = EditArgs {
            name: Some("  lab   3 ".into()),
            ..edit("ws/b")
        };

        let edited = run(&mut core, tmp.path(), thursday_noon(), &args)
            .await
            .unwrap();

        assert_eq!(edited.to_string(), "edited ws/lab 3");
    }

    #[tokio::test]
    async fn an_empty_description_removes_it() {
        let (tmp, mut core) = core().await;
        let set = EditArgs {
            description: Some("sheet 3".into()),
            ..edit("a")
        };
        let clear = EditArgs {
            description: Some(String::new()),
            ..edit("a")
        };

        run(&mut core, tmp.path(), thursday_noon(), &set)
            .await
            .unwrap();
        let description = |core: &Core| core.tree().get(&[0]).unwrap().header.description.clone();
        assert_eq!(description(&core).as_deref(), Some("sheet 3"));
        run(&mut core, tmp.path(), thursday_noon(), &clear)
            .await
            .unwrap();

        assert_eq!(description(&core), None);
    }

    #[tokio::test]
    async fn a_task_gets_a_new_due_date() {
        let (tmp, mut core) = core().await;
        let args = EditArgs {
            due: Some(parse::due("fri 22:00").unwrap()),
            ..edit("a")
        };

        run(&mut core, tmp.path(), thursday_noon(), &args)
            .await
            .unwrap();

        let a = core.tree().get(&[0]).and_then(|n| n.as_task()).unwrap();
        let friday = dt(2026, 10, 16, 22, 0);
        assert_eq!(a.due_date.with_timezone(&Local).naive_local(), friday);
    }

    #[tokio::test]
    async fn a_container_gets_a_new_kind_but_a_task_does_not() {
        let (tmp, mut core) = core().await;
        let ws = EditArgs {
            kind: Some(KindArg::Project),
            ..edit("ws")
        };
        let task = EditArgs {
            kind: Some(KindArg::Project),
            ..edit("a")
        };

        run(&mut core, tmp.path(), thursday_noon(), &ws)
            .await
            .unwrap();

        let kind = core
            .tree()
            .get(&[1])
            .and_then(|n| n.as_container())
            .unwrap()
            .kind;
        assert_eq!(kind, ContainerKind::Project);
        let err = run(&mut core, tmp.path(), thursday_noon(), &task)
            .await
            .err()
            .unwrap();
        assert_eq!(err.to_string(), "--kind is for workspaces and projects");
    }

    #[tokio::test]
    async fn a_container_has_no_due_date() {
        let (tmp, mut core) = core().await;
        let args = EditArgs {
            due: Some(parse::due("fri 22:00").unwrap()),
            ..edit("ws")
        };

        let err = run(&mut core, tmp.path(), thursday_noon(), &args)
            .await
            .err()
            .unwrap();

        assert_eq!(err.to_string(), "--due is for tasks");
    }

    #[tokio::test]
    async fn nothing_to_change_is_an_error() {
        let (tmp, mut core) = core().await;

        let err = run(&mut core, tmp.path(), thursday_noon(), &edit("a"))
            .await
            .err()
            .unwrap();

        assert_eq!(
            err.to_string(),
            "nothing to change: give --name, --description, --due, --estimate or --kind"
        );
    }

    #[tokio::test]
    async fn a_task_gets_its_own_estimate_and_an_empty_one_removes_it() {
        let (tmp, mut core) = core().await;
        let set = EditArgs {
            estimate: Some("1h30".into()),
            ..edit("a")
        };
        let clear = EditArgs {
            estimate: Some(String::new()),
            ..edit("a")
        };

        run(&mut core, tmp.path(), thursday_noon(), &set)
            .await
            .unwrap();
        assert_eq!(estimate_of(&core, &[0]), Some(Minutes::new(90)));
        run(&mut core, tmp.path(), thursday_noon(), &clear)
            .await
            .unwrap();

        assert_eq!(estimate_of(&core, &[0]), None);
    }

    #[tokio::test]
    async fn due_and_estimate_change_together() {
        let (tmp, mut core) = core().await;
        let args = EditArgs {
            due: Some(parse::due("fri 22:00").unwrap()),
            estimate: Some("45m".into()),
            ..edit("a")
        };

        run(&mut core, tmp.path(), thursday_noon(), &args)
            .await
            .unwrap();

        let a = core.tree().get(&[0]).and_then(|n| n.as_task()).unwrap();
        assert_eq!(a.estimate, Some(Minutes::new(45)));
        let friday = dt(2026, 10, 16, 22, 0);
        assert_eq!(a.due_date.with_timezone(&Local).naive_local(), friday);
    }

    #[tokio::test]
    async fn an_estimate_without_unit_is_an_error_and_changes_nothing() {
        let (tmp, mut core) = core().await;
        let args = EditArgs {
            estimate: Some("90".into()),
            ..edit("a")
        };

        let err = run(&mut core, tmp.path(), thursday_noon(), &args)
            .await
            .err()
            .unwrap();

        assert_eq!(
            err.to_string(),
            "write durations like 1h30, 1h30m, 2h or 45m"
        );
        assert_eq!(estimate_of(&core, &[0]), None);
    }

    #[tokio::test]
    async fn a_container_has_no_own_estimate() {
        let (tmp, mut core) = core().await;
        let args = EditArgs {
            estimate: Some("1h".into()),
            ..edit("ws")
        };

        let err = run(&mut core, tmp.path(), thursday_noon(), &args)
            .await
            .err()
            .unwrap();

        assert_eq!(err.to_string(), "--estimate is for tasks");
    }
}
