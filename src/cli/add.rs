//! `udo add task|project|workspace NODE`: the last part of NODE is the new
//! name, the rest names its parent (see `resolve_parent`).

use std::{
    fmt,
    path::{Path, PathBuf, absolute},
};

use chrono::NaiveDateTime;
use serde::Serialize;

use super::{
    parse::{self, Due},
    report::Report,
    resolve::{path_text, resolve_parent},
};
use crate::{
    Res,
    core::Core,
    model::{
        container::{Container, ContainerKind},
        node::Node,
        task::Task,
        time::local_to_fixed,
    },
};

#[derive(clap::Subcommand)]
pub enum AddCommand {
    /// Add a task, e.g. `udo add task "uni/cs/lab 4" --due "fri 22:00"`
    Task(AddTaskArgs),
    /// Add a project, e.g. `udo add project uni/cs`
    Project(AddContainerArgs),
    /// Add a workspace, e.g. `udo add workspace uni --dir ~/uni`
    Workspace(AddContainerArgs),
}

#[derive(clap::Args)]
pub struct AddTaskArgs {
    /// Path of the new task; only a name: in the current folder's container
    pub node: String,
    /// A rule like "fri 22:00" or "+7d 23:59", or "YYYY-MM-DD HH:MM";
    /// default: the container's default deadline
    #[arg(long, value_parser = parse::due)]
    pub due: Option<Due>,
    /// Folder for the task (relative to the current folder)
    #[arg(long, conflicts_with = "no_dir")]
    pub dir: Option<PathBuf>,
    /// No folder, even if the container's task_folders is auto
    #[arg(long)]
    pub no_dir: bool,
    #[arg(long)]
    pub description: Option<String>,
}

#[derive(clap::Args)]
pub struct AddContainerArgs {
    /// Path of the new workspace / project
    pub node: String,
    /// Folder (relative to the current folder); default: below the parent's
    #[arg(long)]
    pub dir: Option<PathBuf>,
    #[arg(long)]
    pub description: Option<String>,
}

/// What `add` created.
#[derive(Serialize)]
pub struct Added {
    /// `task`, `project` or `workspace`.
    pub what: String,
    pub path: String,
    pub dir: Option<PathBuf>,
}

/// Add a task; `now` (local) is where relative due rules start.
pub async fn task(
    core: &mut Core,
    cwd: &Path,
    now: NaiveDateTime,
    args: &AddTaskArgs,
) -> Res<Added> {
    let (parent, name) = resolve_parent(core.tree(), &args.node, cwd)?;
    let due = match args.due {
        Some(due) => due.after(now),
        None => core.task_defaults(&parent, now).due,
    };
    let due = local_to_fixed(due).ok_or("that time doesn't exist (DST switch)")?;
    let dir = match (&args.dir, args.no_dir) {
        // absolute: the parent file stores this path, cwd must not matter
        (Some(dir), _) => Some(absolute(cwd.join(dir))?),
        (None, true) => None,
        (None, false) => core.tree().auto_task_dir(&parent, &name),
    };
    let node =
        Node::task(name, Task::new(dir.clone(), due)).with_description(args.description.clone());
    let path = core.create(&parent, node).await?;
    Ok(Added {
        what: "task".into(),
        path: path_text(core.tree(), &path),
        dir,
    })
}

/// Add a workspace or project.
pub async fn container(
    core: &mut Core,
    cwd: &Path,
    args: &AddContainerArgs,
    kind: ContainerKind,
) -> Res<Added> {
    let (parent, name) = resolve_parent(core.tree(), &args.node, cwd)?;
    let dir = match &args.dir {
        Some(dir) => absolute(cwd.join(dir))?,
        None => core
            .container_dir(&parent, &name)
            .ok_or("the parent has no folder, give --dir")?,
    };
    let node = Node::container(name, Container::new(dir.clone(), kind))
        .with_description(args.description.clone());
    let path = core.create(&parent, node).await?;
    Ok(Added {
        what: kind.to_string(),
        path: path_text(core.tree(), &path),
        dir: Some(dir),
    })
}

impl fmt::Display for Added {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "added {} {}", self.what, self.path)?;
        if let Some(dir) = &self.dir {
            write!(f, "\nfolder: {}", dir.display())?;
        }
        Ok(())
    }
}

impl Report for Added {}

#[cfg(test)]
mod tests {
    use chrono::Local;
    use clap::Parser;

    use super::*;
    use crate::{
        cli::Cli,
        model::settings::{ContainerSettings, TaskFolderSetting},
        test_util::{core, dt, thursday_noon}, // core: disk_tree, root: [a, ws: [b]]
    };

    fn task_args(node: &str) -> AddTaskArgs {
        AddTaskArgs {
            node: node.into(),
            due: None,
            dir: None,
            no_dir: false,
            description: None,
        }
    }

    /// The due date of the task at `path`, as local time.
    fn local_due(core: &Core, path: &[usize]) -> NaiveDateTime {
        let task = core.tree().get(path).and_then(|n| n.as_task()).unwrap();
        task.due_date.with_timezone(&Local).naive_local()
    }

    #[tokio::test]
    async fn a_task_with_a_due_date() {
        let (tmp, mut core) = core().await;
        let args = AddTaskArgs {
            due: Some(parse::due("2026-10-20 09:00").unwrap()),
            ..task_args("ws/lab 4")
        };

        let added = task(&mut core, tmp.path(), thursday_noon(), &args)
            .await
            .unwrap();

        assert_eq!(added.to_string(), "added task ws/lab 4");
        assert_eq!(local_due(&core, &[1, 1]), dt(2026, 10, 20, 9, 0));
    }

    #[tokio::test]
    async fn without_due_the_containers_default_applies() {
        let (tmp, mut core) = core().await;

        task(&mut core, tmp.path(), thursday_noon(), &task_args("ws/lab 4"))
            .await
            .unwrap();

        let default = core.task_defaults(&[1], thursday_noon()).due;
        assert_eq!(local_due(&core, &[1, 1]), default);
    }

    #[tokio::test]
    async fn task_folders_auto_gives_a_folder_unless_no_dir() {
        let (tmp, mut core) = core().await;
        let auto = ContainerSettings {
            task_folders: Some(TaskFolderSetting::Auto),
            ..Default::default()
        };
        core.set_settings(&[1], auto, None).await.unwrap();
        let no_dir = AddTaskArgs {
            no_dir: true,
            ..task_args("ws/lab 5")
        };

        let with = task(&mut core, tmp.path(), thursday_noon(), &task_args("ws/lab 4"))
            .await
            .unwrap();
        let without = task(&mut core, tmp.path(), thursday_noon(), &no_dir)
            .await
            .unwrap();

        let lab_4 = tmp.path().join("ws").join("lab_4");
        assert_eq!(with.dir, Some(lab_4.clone()));
        assert_eq!(with.to_string(), format!("added task ws/lab 4\nfolder: {}", lab_4.display()));
        assert_eq!(without.dir, None);
    }

    #[tokio::test]
    async fn a_project_gets_a_folder_below_its_parent() {
        let (tmp, mut core) = core().await;
        let args = AddContainerArgs {
            node: "ws/cs 101".into(),
            dir: None,
            description: None,
        };

        let added = container(&mut core, tmp.path(), &args, ContainerKind::Project)
            .await
            .unwrap();

        assert_eq!(added.path, "ws/cs 101");
        assert_eq!(added.what, "project");
        assert!(tmp.path().join("ws").join("cs_101").is_dir());
    }

    #[tokio::test]
    async fn a_task_cannot_hold_nodes() {
        let (tmp, mut core) = core().await;

        let err = task(&mut core, tmp.path(), thursday_noon(), &task_args("a/x"))
            .await
            .err()
            .unwrap();

        assert!(err.to_string().contains("not a container"), "{err}");
    }

    #[test]
    fn dir_and_no_dir_exclude_each_other() {
        let parsed = Cli::try_parse_from(["udo", "add", "task", "x", "--dir", "/a", "--no-dir"]);

        assert!(parsed.is_err());
    }
}
