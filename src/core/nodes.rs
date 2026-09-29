//! Creating, editing and removing nodes, and the defaults for new ones.

use std::path::PathBuf;

use chrono::NaiveDateTime;

use crate::{
    Res,
    core::Core,
    dir::default_dir,
    model::{
        NodePath,
        id::NodeId,
        node::{Node, NodePatch},
        sessions::Session,
        settings::TaskFolderSetting,
        time::Time,
        tree::{PurgePlan, PurgeReport, TrashFn},
    },
};

/// What a new task in a container starts with, from inherited settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TaskDefaults {
    /// Local time: the inherited `default_deadline` rule after `now`
    /// (built-in: tomorrow 12:00).
    pub due: NaiveDateTime,
    /// Whether it gets its own folder (inherited `task_folders`, built-in:
    /// none).
    pub task_folders: TaskFolderSetting,
}

impl Core {
    /// Create `node` (a task or a container) in `parent`; returns its path.
    /// The tree checks the name and creates the node's folder.
    pub async fn create(&mut self, parent: &[usize], node: Node) -> Res<NodePath> {
        self.tree.create(parent, node).await
    }

    /// Apply `patch` to the node at `path` (name checks as for `create`).
    pub async fn edit(&mut self, path: &[usize], patch: NodePatch) -> Res<()> {
        self.tree.edit(path, patch).await
    }

    /// Unregister the node at `path`; its files stay on disk. A timer on it
    /// or on a task below it is stopped first (at `at`) and returned; the
    /// session keeps its task's data. Callers pass `time::now()`.
    pub async fn delete(&mut self, path: &[usize], at: Time) -> Res<Option<Session>> {
        let tasks = self.removable_tasks(path)?;
        let stopped = self.stop_if_on(&tasks, at).await?;
        self.tree.delete(path).await?;
        Ok(stopped)
    }

    /// What a full delete of `path` would move to the Trash (None: no
    /// folder). Err: the root, a missing path, or a failed safety check.
    pub fn purge_plan(&self, path: &[usize]) -> Res<Option<PurgePlan>> {
        self.tree.purge_plan(path)
    }

    /// Execute `plan`: unregister the node, then trash its folders. Stops a
    /// timer on it first, like `delete`.
    pub async fn purge(
        &mut self,
        plan: &PurgePlan,
        trash: TrashFn,
        at: Time,
    ) -> Res<(PurgeReport, Option<Session>)> {
        let tasks = self.removable_tasks(&plan.path)?;
        let stopped = self.stop_if_on(&tasks, at).await?;
        let report = self.tree.purge(plan, trash).await?;
        Ok((report, stopped))
    }

    /// The ids of the task at `path` or of every task below the container
    /// there. Err for the root or a missing path, so nothing is stopped for
    /// a remove that cannot happen.
    fn removable_tasks(&self, path: &[usize]) -> Res<Vec<NodeId>> {
        if path.is_empty() {
            return Err("the root cannot be removed".into());
        }
        let node = self.tree.get(path).ok_or("no node at the path")?;
        if node.as_task().is_some() {
            return Ok(vec![node.id()]);
        }
        Ok(self
            .tree
            .rows()
            .into_iter()
            .filter(|r| r.path.starts_with(path) && r.node.as_task().is_some())
            .map(|r| r.node.id())
            .collect())
    }

    /// Defaults for a new task in `parent`, from its inherited settings.
    pub fn task_defaults(&self, parent: &[usize], now: NaiveDateTime) -> TaskDefaults {
        let task_folders = self
            .tree
            .setting(parent, |s| s.task_folders)
            .map_or(TaskFolderSetting::None, |r| r.value);
        let due = self
            .tree
            .setting(parent, |s| s.default_deadline)
            .map_or(now, |r| r.value.next_after(now));
        TaskDefaults { due, task_folders }
    }

    /// Default folder of a new container `name` in `parent`:
    /// `<parent dir>/<folder name>`. None: `parent` has no folder (a task,
    /// missing) or the name gives none.
    pub fn container_dir(&self, parent: &[usize], name: &str) -> Option<PathBuf> {
        default_dir(self.tree.get(parent)?.dir()?, name)
    }
}

#[cfg(test)]
mod tests {
    use chrono::{FixedOffset, TimeZone};

    use super::*;
    use crate::{
        model::{
            container::{Container, ContainerKind},
            sessions::SessionStore,
            settings::ContainerSettings,
            task::Task,
        },
        test_util::{at, core, dt, thursday_noon},
    };

    fn due() -> Time {
        FixedOffset::east_opt(2 * 3600)
            .unwrap()
            .with_ymd_and_hms(2026, 10, 16, 22, 0, 0)
            .unwrap()
    }

    #[tokio::test]
    async fn create_adds_a_task_with_its_values() {
        let (_tmp, mut core) = core().await;
        let lab = Node::task("lab 4".into(), Task::new(None, due()))
            .with_description(Some("sheet 4".into()));

        let path = core.create(&[1], lab).await.unwrap();

        let node = core.tree().get(&path).unwrap();
        assert_eq!(node.name(), "lab 4");
        assert_eq!(node.header.description.as_deref(), Some("sheet 4"));
        assert_eq!(node.as_task().unwrap().due_date, due());
    }

    #[tokio::test]
    async fn create_makes_a_containers_folder() {
        let (tmp, mut core) = core().await;
        let dir = tmp.path().join("ws").join("cs");
        let cs = Node::container("cs".into(), Container::new(dir.clone(), ContainerKind::Project));

        let path = core.create(&[1], cs).await.unwrap();

        assert_eq!(core.tree().get(&path).unwrap().name(), "cs");
        assert!(dir.is_dir());
    }

    #[tokio::test]
    async fn delete_unregisters_the_node() {
        let (_tmp, mut core) = core().await;

        core.delete(&[0], due()).await.unwrap();

        assert_eq!(core.tree().get(&[0]).unwrap().name(), "ws"); // "a" is gone
    }

    #[tokio::test]
    async fn deleting_the_timed_task_stops_the_timer() {
        let (_tmp, mut core) = core().await;
        core.start(&[0], at(14, 0)).await.unwrap();

        let stopped = core.delete(&[0], at(14, 45)).await.unwrap();

        assert_eq!(stopped.unwrap().end, Some(at(14, 45)));
        assert_eq!(core.sessions().running().await.unwrap(), None);
    }

    /// "b" runs inside "ws": removing "ws" stops it.
    #[tokio::test]
    async fn deleting_a_container_stops_a_timer_below_it() {
        let (_tmp, mut core) = core().await;
        core.start(&[1, 0], at(14, 0)).await.unwrap();

        let stopped = core.delete(&[1], at(14, 45)).await.unwrap();

        assert_eq!(stopped.unwrap().task.name, "b");
    }

    #[tokio::test]
    async fn deleting_another_node_keeps_the_timer() {
        let (_tmp, mut core) = core().await;
        core.start(&[1, 0], at(14, 0)).await.unwrap();

        let stopped = core.delete(&[0], at(14, 45)).await.unwrap();

        assert_eq!(stopped, None);
        assert!(core.sessions().running().await.unwrap().is_some());
    }

    /// A remove that cannot happen stops nothing.
    #[tokio::test]
    async fn a_refused_delete_keeps_the_timer() {
        let (_tmp, mut core) = core().await;
        core.start(&[0], at(14, 0)).await.unwrap();

        assert!(core.delete(&[], at(14, 45)).await.is_err()); // the root
        assert!(core.delete(&[7], at(14, 45)).await.is_err()); // missing

        assert!(core.sessions().running().await.unwrap().is_some());
    }

    #[tokio::test]
    async fn purging_stops_a_timer_below_the_node() {
        let (_tmp, mut core) = core().await;
        core.start(&[1, 0], at(14, 0)).await.unwrap();
        let plan = core.purge_plan(&[1]).unwrap().unwrap();

        let (_report, stopped) = core.purge(&plan, |_| Ok(()), at(14, 45)).await.unwrap();

        assert_eq!(stopped.unwrap().task.name, "b");
    }

    #[tokio::test]
    async fn task_defaults_come_from_the_inherited_settings() {
        let (_tmp, mut core) = core().await;
        let root = ContainerSettings {
            default_deadline: Some("fri 22:00".parse().unwrap()),
            task_folders: Some(TaskFolderSetting::Auto),
            ..Default::default()
        };
        core.set_settings(&[], root, None).await.unwrap();

        let defaults = core.task_defaults(&[1], thursday_noon()); // ws inherits

        let friday_10pm = dt(2026, 10, 16, 22, 0);
        assert_eq!(
            defaults,
            TaskDefaults {
                due: friday_10pm,
                task_folders: TaskFolderSetting::Auto
            }
        );
    }

    /// Nothing set anywhere: the built-in defaults (tomorrow 12:00, no task
    /// folder), as the TUI form had.
    #[tokio::test]
    async fn task_defaults_without_settings_are_the_builtin_ones() {
        let (_tmp, core) = core().await;

        let defaults = core.task_defaults(&[1], thursday_noon());

        assert_eq!(defaults.due, dt(2026, 10, 16, 12, 0)); // friday noon
        assert_eq!(defaults.task_folders, TaskFolderSetting::None);
    }

    #[tokio::test]
    async fn container_dir_is_below_the_parents_folder() {
        let (tmp, core) = core().await;

        let dir = core.container_dir(&[1], "cs 101");

        assert_eq!(dir, Some(tmp.path().join("ws").join("cs_101")));
    }

    #[tokio::test]
    async fn container_dir_under_a_task_or_nothing_is_none() {
        let (_tmp, core) = core().await;

        assert_eq!(core.container_dir(&[0], "x"), None); // "a" has no folder
        assert_eq!(core.container_dir(&[7], "x"), None); // no such node
    }
}
