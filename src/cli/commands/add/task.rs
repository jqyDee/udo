//! `udo add task NODE [--due] [--dir | --no-dir] [--description]`.

use std::path::{Path, PathBuf, absolute};

use chrono::NaiveDateTime;

use super::Added;
use crate::{
    Res,
    cli::{
        parse::{self, Due},
        resolve::{path_text, resolve_parent},
    },
    core::Core,
    model::{node::Node, task::Task, time::local_to_fixed},
};

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

/// Add a task; `now` (local) is where relative due rules start.
pub async fn run(
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

        let added = run(&mut core, tmp.path(), thursday_noon(), &args)
            .await
            .unwrap();

        assert_eq!(added.to_string(), "added task ws/lab 4");
        assert_eq!(local_due(&core, &[1, 1]), dt(2026, 10, 20, 9, 0));
    }

    #[tokio::test]
    async fn without_due_the_containers_default_applies() {
        let (tmp, mut core) = core().await;

        run(&mut core, tmp.path(), thursday_noon(), &task_args("ws/lab 4"))
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

        let with = run(&mut core, tmp.path(), thursday_noon(), &task_args("ws/lab 4"))
            .await
            .unwrap();
        let without = run(&mut core, tmp.path(), thursday_noon(), &no_dir)
            .await
            .unwrap();

        let lab_4 = tmp.path().join("ws").join("lab_4");
        assert_eq!(with.dir, Some(lab_4.clone()));
        assert_eq!(with.to_string(), format!("added task ws/lab 4\nfolder: {}", lab_4.display()));
        assert_eq!(without.dir, None);
    }

    #[tokio::test]
    async fn a_task_cannot_hold_nodes() {
        let (tmp, mut core) = core().await;

        let err = run(&mut core, tmp.path(), thursday_noon(), &task_args("a/x"))
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
