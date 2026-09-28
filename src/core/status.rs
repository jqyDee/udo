//! Task status (to do, in progress, done, …).

use crate::{
    Res,
    core::Core,
    model::{sessions::Session, task::TaskStatus, time::Time},
};

impl Core {
    /// Set the status of the task at `path` and save it. Not a task: error.
    /// Finishing the task the timer runs on stops the timer first (at `at`);
    /// that session is returned. Callers pass `time::now()`.
    pub async fn set_status(
        &mut self,
        path: &[usize],
        status: TaskStatus,
        at: Time,
    ) -> Res<Option<Session>> {
        let Some(id) = self
            .tree
            .get(path)
            .and_then(|n| n.as_task().map(|_| n.id()))
        else {
            return Err("only tasks have a status".into()); // checked before stopping anything
        };
        let stopped = match status {
            TaskStatus::Finished => self.stop_if_on(&[id], at).await?,
            _ => None,
        };
        self.tree.set_task_status(path, status).await?;
        Ok(stopped)
    }
}

#[cfg(test)]
mod tests {
    use chrono::{FixedOffset, TimeZone};

    use crate::{
        core::Core,
        model::{node::Node, sessions::SessionStore, task::TaskStatus, time::Time, tree::Tree},
        storage::Storage,
        test_util::disk_tree,
    };

    fn at(h: u32, m: u32) -> Time {
        FixedOffset::east_opt(2 * 3600)
            .unwrap()
            .with_ymd_and_hms(2026, 10, 15, h, m, 0)
            .unwrap()
    }

    async fn core() -> (tempfile::TempDir, Core) {
        let (tmp, tree) = disk_tree().await; // root: [a, ws: [b]]
        (tmp, Core::new(tree, Storage::in_memory()))
    }

    #[tokio::test]
    async fn the_status_is_set_and_saved() {
        let (tmp, mut core) = core().await;

        core.set_status(&[0], TaskStatus::Finished, at(14, 0))
            .await
            .unwrap();

        let reloaded = Tree::load_from(tmp.path()).await.unwrap();
        let a = reloaded.get(&[0]).and_then(Node::as_task).unwrap();
        assert_eq!(a.status, TaskStatus::Finished);
    }

    #[tokio::test]
    async fn containers_have_no_status() {
        let (_tmp, mut core) = core().await;

        let result = core.set_status(&[1], TaskStatus::Finished, at(14, 0)).await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn finishing_the_timed_task_stops_the_timer() {
        let (_tmp, mut core) = core().await;
        core.start(&[0], at(14, 0)).await.unwrap();

        let stopped = core
            .set_status(&[0], TaskStatus::Finished, at(14, 45))
            .await
            .unwrap();

        assert_eq!(stopped.unwrap().end, Some(at(14, 45)));
        assert_eq!(core.sessions().running().await.unwrap(), None);
    }

    #[tokio::test]
    async fn finishing_another_task_keeps_the_timer() {
        let (_tmp, mut core) = core().await;
        core.start(&[0], at(14, 0)).await.unwrap();

        let stopped = core
            .set_status(&[1, 0], TaskStatus::Finished, at(14, 45))
            .await
            .unwrap();

        assert_eq!(stopped, None);
        assert!(core.sessions().running().await.unwrap().is_some());
    }

    /// Only "done" stops it: other statuses leave the timer alone.
    #[tokio::test]
    async fn other_statuses_keep_the_timer() {
        let (_tmp, mut core) = core().await;
        core.start(&[0], at(14, 0)).await.unwrap();

        core.set_status(&[0], TaskStatus::Stale, at(14, 45))
            .await
            .unwrap();

        assert!(core.sessions().running().await.unwrap().is_some());
    }
}
