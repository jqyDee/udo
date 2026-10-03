//! The one timer: started and stopped by hand (`s`, `udo start / stop`) or
//! by programs (`udo track`). Programs have an owner and can only stop their
//! own session; by hand always wins.

use std::fmt;

use crate::{
    Res,
    core::Core,
    model::{
        id::NodeId,
        node::Node,
        sessions::{Owner, Session, SessionSource, SessionStore, TaskRef},
        time::Time,
    },
};

/// `start` on a done task (the task's name). A type of its own, so the TUI
/// and the CLI can add how to reopen it there.
#[derive(Debug)]
pub struct IsDone(pub String);

impl fmt::Display for IsDone {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} is done", self.0)
    }
}

impl std::error::Error for IsDone {}

impl Core {
    /// Start timing the task at `path` at `at` by hand (`s`, `udo start`):
    /// source and owner `manual`. A running session is stopped first,
    /// whoever owns it. Checks: see `start_as`. Callers pass `time::now()`.
    pub async fn start(&mut self, path: &[usize], at: Time) -> Res<Session> {
        self.start_as(path, SessionSource::Manual, Owner::manual(), at)
            .await
    }

    /// Start timing for a program (`udo track start`): like `start`, with
    /// the program's source and owner. Takes over whatever runs; the same
    /// task already running is a no-op and keeps its owner (a timer started
    /// by hand is not handed to the program). Owner `manual` is refused:
    /// programs cannot pose as the user. Callers pass `time::now()`.
    pub async fn track_start(
        &mut self,
        path: &[usize],
        source: SessionSource,
        owner: Owner,
        at: Time,
    ) -> Res<Session> {
        refuse_manual(&owner)?;
        self.start_as(path, source, owner, at).await
    }

    /// What `start` and `track_start` share: the checks, then the store's
    /// `start`. The session itself makes the task "started". A done task is
    /// refused (`IsDone`: reopen it first); a container, the root or a
    /// missing path is not a task. Private: the only way past
    /// `refuse_manual` is `start`.
    async fn start_as(
        &mut self,
        path: &[usize],
        source: SessionSource,
        owner: Owner,
        at: Time,
    ) -> Res<Session> {
        let task = self.task_ref(path)?;
        if self
            .tree
            .get(path)
            .and_then(Node::as_task)
            .is_some_and(|t| t.done_at.is_some())
        {
            return Err(IsDone(task.name).into());
        }
        Ok(self.storage.sessions.start(task, source, owner, at).await?)
    }

    /// What a session on the task at `path` remembers of it. Not a task
    /// (a container, the root): an error.
    pub(super) fn task_ref(&self, path: &[usize]) -> Res<TaskRef> {
        // the root row has no parent
        let (_, parent_path) = path.split_last().ok_or("the root is not a task")?;
        let node = self.tree.get(path).ok_or("no such node")?;
        let parent = self.tree.get(parent_path).ok_or("no such node")?;
        Ok(TaskRef::of(node, parent).ok_or_else(|| format!("{:?} is not a task", node.name()))?)
    }

    /// Stop the running session at `at`, if any, whoever owns it (by hand
    /// always wins). Callers pass `time::now()`.
    pub async fn stop(&self, at: Time) -> Res<Option<Session>> {
        Ok(self.storage.sessions.stop(None, at).await?)
    }

    /// Stop the running session at `at` if `owner` owns it (`udo track
    /// stop`). Someone else's session or nothing running: `Ok(None)`, not an
    /// error (a server-wide tmux hook fires for every session). Owner
    /// `manual` is refused, like in `track_start`. Callers pass
    /// `time::now()`.
    pub async fn track_stop(&self, owner: &Owner, at: Time) -> Res<Option<Session>> {
        refuse_manual(owner)?;
        Ok(self.storage.sessions.stop(Some(owner), at).await?)
    }

    /// Stop the running session at `at` if it times one of `tasks` (a task
    /// that is finished or removed); another task's timer keeps running.
    pub(super) async fn stop_if_on(&self, tasks: &[NodeId], at: Time) -> Res<Option<Session>> {
        match self.storage.sessions.running().await? {
            Some(running) if tasks.contains(&running.task.id) => self.stop(at).await,
            _ => Ok(None),
        }
    }

    /// The session running right now, if any (one timer).
    pub async fn running_session(&self) -> Res<Option<Session>> {
        Ok(self.storage.sessions.running().await?)
    }
}

/// `track_start` / `track_stop` with owner `manual`: that owner belongs to
/// `s` / `udo start`, so a program could otherwise stop or keep a timer
/// started by hand.
fn refuse_manual(owner: &Owner) -> Res<()> {
    if owner.is_manual() {
        return Err("owner \"manual\" is reserved for starting by hand".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::{
        core::Core,
        core::IsDone,
        model::sessions::{Owner, Session, SessionQuery, SessionSource, SessionStore},
        test_util::{at, core},
    };

    async fn all(core: &Core) -> Vec<Session> {
        core.sessions()
            .query(&SessionQuery::default())
            .await
            .unwrap()
    }

    /// A program's source and owner.
    fn tmux(owner: &str) -> (SessionSource, Owner) {
        ("tmux".parse().unwrap(), owner.parse().unwrap())
    }

    #[tokio::test]
    async fn start_times_the_task() {
        let (tmp, mut core) = core().await;

        let session = core.start(&[1, 0], at(14, 0)).await.unwrap();

        assert_eq!(session.task.name, "b");
        assert_eq!(
            session.task.container_id,
            core.tree().get(&[1]).unwrap().id()
        );
        assert_eq!(session.task.container_dir, tmp.path().join("ws"));
        assert_eq!(session.start, at(14, 0));
        assert_eq!(core.sessions().running().await.unwrap(), Some(session));
    }

    #[tokio::test]
    async fn a_done_task_is_refused() {
        let (_tmp, mut core) = core().await;
        core.set_done(&[0], true, at(13, 0)).await.unwrap();

        let err = core.start(&[0], at(14, 0)).await.unwrap_err();

        assert_eq!(err.to_string(), "a is done");
        assert!(err.downcast_ref::<IsDone>().is_some());
        assert!(all(&core).await.is_empty());
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

    // --------------- by programs (`udo track`) ---------------

    #[tokio::test]
    async fn start_by_hand_is_manual() {
        let (_tmp, mut core) = core().await;

        let session = core.start(&[0], at(14, 0)).await.unwrap();

        assert_eq!(
            (session.source, session.owner),
            (SessionSource::Manual, Owner::manual())
        );
    }

    #[tokio::test]
    async fn track_start_records_source_and_owner() {
        let (_tmp, mut core) = core().await;
        let (source, owner) = tmux("tmux:a");

        let session = core
            .track_start(&[0], source.clone(), owner.clone(), at(14, 0))
            .await
            .unwrap();

        assert_eq!(
            (session.source.clone(), session.owner.clone()),
            (source, owner)
        );
        assert_eq!(core.sessions().running().await.unwrap(), Some(session));
    }

    #[tokio::test]
    async fn track_with_the_manual_owner_is_refused() {
        let (_tmp, mut core) = core().await;
        let (source, _) = tmux("tmux:a");
        core.start(&[0], at(14, 0)).await.unwrap();

        let start = core.track_start(&[1, 0], source, Owner::manual(), at(15, 0));
        assert!(start.await.is_err());
        assert!(core.track_stop(&Owner::manual(), at(15, 0)).await.is_err());

        let running = core.sessions().running().await.unwrap().unwrap();
        assert_eq!(running.task.name, "a"); // neither taken over nor stopped
    }

    #[tokio::test]
    async fn track_stop_ends_only_its_own_session() {
        let (_tmp, mut core) = core().await;
        let (source, owner) = tmux("tmux:a");
        core.track_start(&[0], source, owner.clone(), at(14, 0))
            .await
            .unwrap();

        let other = tmux("tmux:b").1;
        assert_eq!(core.track_stop(&other, at(15, 0)).await.unwrap(), None);
        assert!(core.sessions().running().await.unwrap().is_some());

        let stopped = core.track_stop(&owner, at(16, 0)).await.unwrap().unwrap();
        assert_eq!(stopped.end, Some(at(16, 0)));
        assert_eq!(core.sessions().running().await.unwrap(), None);
    }

    #[tokio::test]
    async fn stop_by_hand_ends_a_program_session() {
        let (_tmp, mut core) = core().await;
        let (source, owner) = tmux("tmux:a");
        core.track_start(&[0], source, owner, at(14, 0))
            .await
            .unwrap();

        let stopped = core.stop(at(15, 0)).await.unwrap().unwrap();

        assert_eq!(stopped.end, Some(at(15, 0)));
        assert_eq!(core.sessions().running().await.unwrap(), None);
    }

    /// `track_start` goes through the same checks as `start`.
    #[tokio::test]
    async fn track_start_refuses_done_tasks_and_containers() {
        let (_tmp, mut core) = core().await;
        core.set_done(&[0], true, at(13, 0)).await.unwrap();

        for path in [&[0][..], &[], &[1], &[7]] {
            let (source, owner) = tmux("tmux:a");
            let err = core.track_start(path, source, owner, at(14, 0)).await;
            assert!(err.is_err(), "{path:?}");
        }
        let (source, owner) = tmux("tmux:a");
        let err = core
            .track_start(&[0], source, owner, at(14, 0))
            .await
            .unwrap_err();
        assert!(err.downcast_ref::<IsDone>().is_some());

        assert!(all(&core).await.is_empty());
    }
}
