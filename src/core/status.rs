//! Task status: only "done" is stored; "started" / "to do" come from the
//! sessions (`Task::status`), so the views ask `tasks_with_sessions`.

use std::collections::HashSet;

use crate::{
    Res,
    core::Core,
    model::{
        id::NodeId,
        sessions::{Session, SessionQuery, SessionStore},
        time::Time,
    },
};

impl Core {
    /// Mark the task at `path` done at `now` (`done`) or reopen it, and save
    /// it. Not a task: error. Marking the task the timer runs on done stops
    /// the timer first (at `now`); that session is returned. Callers pass
    /// `time::now()`.
    pub async fn set_done(
        &mut self,
        path: &[usize],
        done: bool,
        now: Time,
    ) -> Res<Option<Session>> {
        let Some(id) = self
            .tree
            .get(path)
            .and_then(|n| n.as_task().map(|_| n.id()))
        else {
            return Err("only tasks can be done".into()); // checked before stopping anything
        };
        let stopped = match done {
            true => self.stop_if_on(&[id], now).await?,
            false => None,
        };
        self.tree.set_task_done(path, done.then_some(now)).await?;
        Ok(stopped)
    }

    /// Ids of the tasks with at least one (not removed) session, running
    /// ones included.
    pub async fn tasks_with_sessions(&self) -> Res<HashSet<NodeId>> {
        let sessions = self
            .storage
            .sessions
            .query(&SessionQuery::default())
            .await?;
        Ok(sessions.into_iter().map(|s| s.task.id).collect())
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        model::{node::Node, sessions::SessionStore, tree::Tree},
        test_util::{at, core}, // core: disk_tree, root: [a, ws: [b]]
    };

    #[tokio::test]
    async fn done_is_set_and_saved() {
        let (tmp, mut core) = core().await;

        core.set_done(&[0], true, at(14, 0)).await.unwrap();

        let reloaded = Tree::load_from(tmp.path()).await.unwrap();
        let a = reloaded.get(&[0]).and_then(Node::as_task).unwrap();
        assert_eq!(a.done_at, Some(at(14, 0)));
    }

    #[tokio::test]
    async fn undo_clears_done() {
        let (_tmp, mut core) = core().await;
        core.set_done(&[0], true, at(14, 0)).await.unwrap();

        core.set_done(&[0], false, at(15, 0)).await.unwrap();

        let a = core.tree().get(&[0]).and_then(Node::as_task).unwrap();
        assert_eq!(a.done_at, None);
    }

    #[tokio::test]
    async fn containers_cannot_be_done() {
        let (_tmp, mut core) = core().await;

        let result = core.set_done(&[1], true, at(14, 0)).await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn done_on_the_timed_task_stops_the_timer() {
        let (_tmp, mut core) = core().await;
        core.start(&[0], at(14, 0)).await.unwrap();

        let stopped = core.set_done(&[0], true, at(14, 45)).await.unwrap();

        assert_eq!(stopped.unwrap().end, Some(at(14, 45)));
        assert_eq!(core.sessions().running().await.unwrap(), None);
    }

    #[tokio::test]
    async fn done_on_another_task_keeps_the_timer() {
        let (_tmp, mut core) = core().await;
        core.start(&[0], at(14, 0)).await.unwrap();

        let stopped = core.set_done(&[1, 0], true, at(14, 45)).await.unwrap();

        assert_eq!(stopped, None);
        assert!(core.sessions().running().await.unwrap().is_some());
    }

    #[tokio::test]
    async fn tasks_with_sessions_follows_the_sessions() {
        let (_tmp, core) = core().await;
        let a = core.tree().get(&[0]).unwrap().id();
        let b = core.tree().get(&[1, 0]).unwrap().id();
        assert!(core.tasks_with_sessions().await.unwrap().is_empty());

        let s = core
            .add_session(&[0], at(9, 0), at(10, 0), at(20, 0))
            .await
            .unwrap();
        let with = core.tasks_with_sessions().await.unwrap();
        assert!(with.contains(&a) && !with.contains(&b));

        core.delete_session(s.id).await.unwrap(); // its last session
        assert!(!core.tasks_with_sessions().await.unwrap().contains(&a));
    }

    #[tokio::test]
    async fn a_running_session_counts() {
        let (_tmp, mut core) = core().await;
        let a = core.tree().get(&[0]).unwrap().id();

        core.start(&[0], at(14, 0)).await.unwrap();

        assert!(core.tasks_with_sessions().await.unwrap().contains(&a));
    }
}
