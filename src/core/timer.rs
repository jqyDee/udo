//! Timing tasks by hand: start and stop the one timer.

use crate::{
    Res,
    core::Core,
    model::{
        sessions::{Session, SessionSource, SessionStore, TaskRef},
        task::TaskStatus,
        time::Time,
    },
};

impl Core {
    /// Start timing the task at `path` at `at` (a running one is stopped
    /// first) and mark it in progress. The session comes first: a failed
    /// status write never loses recorded time. Callers pass `time::now()`.
    pub async fn start(&mut self, path: &[usize], at: Time) -> Res<Session> {
        let (_, parent_path) = path.split_last().ok_or("not a task")?; // the root row has no parent
        let task = TaskRef::of(
            self.tree.get(path).ok_or("no such node")?,
            self.tree.get(parent_path).ok_or("no such node")?,
        )
        .ok_or("not a task")?;

        let session = self
            .storage
            .sessions
            .start(task, SessionSource::Manual, at)
            .await?;
        self.tree
            .set_task_status(path, TaskStatus::InProgress)
            .await?;
        Ok(session)
    }

    /// Stop the running session at `at`, if any. Callers pass `time::now()`.
    pub async fn stop(&self, at: Time) -> Res<Option<Session>> {
        Ok(self.storage.sessions.stop(at).await?)
    }
}

#[cfg(test)]
mod tests {
    use chrono::{FixedOffset, TimeZone};

    use crate::{
        core::Core,
        model::{
            node::Node,
            sessions::{Session, SessionQuery, SessionStore},
            task::TaskStatus,
            time::Time,
            tree::Tree,
        },
        storage::Storage,
        test_util::disk_tree,
    };

    /// 2026-10-15 at `h:m`, offset +02:00.
    fn at(h: u32, m: u32) -> Time {
        FixedOffset::east_opt(2 * 3600)
            .unwrap()
            .with_ymd_and_hms(2026, 10, 15, h, m, 0)
            .unwrap()
    }

    async fn core() -> (tempfile::TempDir, Core) {
        let (tmp, tree) = disk_tree().await;
        (tmp, Core::new(tree, Storage::in_memory()))
    }

    fn status(tree: &Tree, path: &[usize]) -> TaskStatus {
        tree.get(path).and_then(Node::as_task).unwrap().status
    }

    async fn all(core: &Core) -> Vec<Session> {
        core.sessions()
            .query(&SessionQuery::default())
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn start_times_the_task_and_marks_it_in_progress() {
        let (tmp, mut core) = core().await;

        let session = core.start(&[1, 0], at(14, 0)).await.unwrap();

        assert_eq!(session.task.name, "b");
        assert_eq!(session.task.container_id, core.tree().get(&[1]).unwrap().id());
        assert_eq!(session.task.container_dir, tmp.path().join("ws"));
        assert_eq!(session.start, at(14, 0));
        assert_eq!(core.sessions().running().await.unwrap(), Some(session));
        assert_eq!(status(core.tree(), &[1, 0]), TaskStatus::InProgress);
    }

    #[tokio::test]
    async fn the_status_is_saved() {
        let (tmp, mut core) = core().await;

        core.start(&[0], at(14, 0)).await.unwrap();

        let reloaded = Tree::load_from(tmp.path()).await.unwrap();
        assert_eq!(status(&reloaded, &[0]), TaskStatus::InProgress);
    }

    /// A task directly in the root: the root is its container.
    #[tokio::test]
    async fn a_task_in_the_root_belongs_to_the_root() {
        let (tmp, mut core) = core().await;

        let session = core.start(&[0], at(14, 0)).await.unwrap();

        assert_eq!(session.task.container_id, core.tree().root.id());
        assert_eq!(session.task.container_dir, tmp.path());
    }

    #[tokio::test]
    async fn starting_another_task_stops_the_first() {
        let (_tmp, mut core) = core().await;
        let first = core.start(&[0], at(14, 0)).await.unwrap();

        let second = core.start(&[1, 0], at(15, 0)).await.unwrap();

        assert_eq!(core.sessions().running().await.unwrap(), Some(second));
        let first = all(&core).await.into_iter().find(|s| s.id == first.id);
        assert_eq!(first.unwrap().end, Some(at(15, 0)));
        // stopped, not finished: it stays in progress
        assert_eq!(status(core.tree(), &[0]), TaskStatus::InProgress);
    }

    #[tokio::test]
    async fn the_root_and_containers_cannot_be_started() {
        let (_tmp, mut core) = core().await;

        for path in [&[][..], &[1]] {
            assert!(core.start(path, at(14, 0)).await.is_err());
        }

        assert!(all(&core).await.is_empty());
    }

    #[tokio::test]
    async fn a_missing_path_is_an_error() {
        let (_tmp, mut core) = core().await;

        assert!(core.start(&[7], at(14, 0)).await.is_err());
        assert!(all(&core).await.is_empty());
    }

    /// The session comes first: a start the store refuses leaves the status
    /// alone.
    #[tokio::test]
    async fn a_refused_start_keeps_the_status() {
        let (_tmp, mut core) = core().await;
        core.start(&[0], at(14, 0)).await.unwrap();
        core.stop(at(15, 0)).await.unwrap();

        let inside = core.start(&[1, 0], at(14, 30)).await;

        assert!(inside.is_err()); // over the recorded 14:00-15:00
        assert_eq!(status(core.tree(), &[1, 0]), TaskStatus::Pending);
    }

    #[tokio::test]
    async fn stop_ends_the_running_session() {
        let (_tmp, mut core) = core().await;
        core.start(&[0], at(14, 0)).await.unwrap();

        let stopped = core.stop(at(15, 0)).await.unwrap().unwrap();

        assert_eq!(stopped.end, Some(at(15, 0)));
        assert_eq!(core.sessions().running().await.unwrap(), None);
    }

    #[tokio::test]
    async fn stop_without_a_running_session_is_none() {
        let (_tmp, core) = core().await;

        assert_eq!(core.stop(at(15, 0)).await.unwrap(), None);
    }
}
