//! `udo edit [NODE] --name … --description … --due … --kind …`: change a
//! node's own fields. Only what is given changes.

use std::{fmt, path::Path};

use chrono::NaiveDateTime;
use serde::Serialize;

use super::{
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
        time::local_to_fixed,
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
        && args.kind.is_none()
    {
        return Err("nothing to change: give --name, --description, --due or --kind".into());
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
    if args.kind.is_some() && is_task {
        return Err("--kind is for workspaces and projects".into());
    }
    let due = match args.due {
        Some(due) => {
            Some(local_to_fixed(due.after(now)).ok_or("that time doesn't exist (DST switch)")?)
        }
        None => None,
    };
    let body = match (due, args.kind) {
        (Some(_), Some(_)) => {
            unreachable!("--due and --kind never fit the same node (checked above)")
        }
        (Some(due_date), None) => Some(BodyPatch::Task(TaskPatch {
            due_date: Some(due_date),
            ..Default::default()
        })),
        (None, Some(kind)) => Some(BodyPatch::Container(ContainerPatch {
            kind: Some(kind.into()),
            ..Default::default()
        })),
        (None, None) => None,
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
            kind: None,
        }
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

        assert!(err.to_string().contains("nothing to change"), "{err}");
    }
}
