//! Reading and correcting recorded time: the sessions of a node; add, edit,
//! split, cut and delete sessions. Thin: the store keeps its own rules
//! (overlap, outside, running); Core adds "not in the future" (`not_after`),
//! which needs a `now` the store does not have.

use crate::{
    Res,
    core::Core,
    model::{
        sessions::{Session, SessionId, SessionPatch, SessionQuery, SessionStore},
        time::Time,
    },
};

/// `split_session` gave `None`: the time is on the session's start or end.
/// Shared by the CLI and the TUI.
pub const SPLIT_AT_EDGE: &str = "that is the session's start or end: nothing to split";

impl Core {
    /// Sessions of the node at `path` (a task: its own; a container: every
    /// task below it), oldest first, removed ones left out. No such node:
    /// none.
    pub async fn sessions_of(&self, path: &[usize]) -> Res<Vec<Session>> {
        let query = SessionQuery {
            tasks: Some(self.tree.task_ids_below(path)),
            ..Default::default()
        };
        Ok(self.storage.sessions.query(&query).await?)
    }

    /// Record a session on the task at `path` by hand (source `manual`). A
    /// time after `now` is refused, before the store is touched. Callers
    /// pass `time::now()`.
    pub async fn add_session(
        &self,
        path: &[usize],
        start: Time,
        end: Time,
        now: Time,
    ) -> Res<Session> {
        not_after(now, &[Some(start), Some(end)])?;
        let task = self.task_ref(path)?;
        Ok(self.storage.sessions.add(task, start, end).await?)
    }

    /// Move a session's start and / or end. A time after `now` is refused,
    /// before the store is touched. Callers pass `time::now()`.
    pub async fn edit_session(&self, id: SessionId, patch: SessionPatch, now: Time) -> Res<()> {
        not_after(now, &[patch.start, patch.end])?;
        Ok(self.storage.sessions.edit(id, patch).await?)
    }

    /// Split a session in two at `at`; `at` on its start or end: `None`.
    pub async fn split_session(&self, id: SessionId, at: Time) -> Res<Option<(Session, Session)>> {
        Ok(self.storage.sessions.split(id, at).await?)
    }

    /// Remove `[from, to)` from a session; returns what is left. `from` or
    /// `to` after `now` is refused, before the store is touched. Callers
    /// pass `time::now()`.
    pub async fn cut_session(
        &self,
        id: SessionId,
        from: Time,
        to: Time,
        now: Time,
    ) -> Res<Vec<Session>> {
        not_after(now, &[Some(from), Some(to)])?;
        Ok(self.storage.sessions.cut(id, from, to).await?)
    }

    /// Soft delete: hidden, the data stays.
    pub async fn delete_session(&self, id: SessionId) -> Res<()> {
        Ok(self.storage.sessions.delete(id).await?)
    }
}

/// Sessions record what happened: no time of a session after `now`.
const IN_FUTURE: &str = "that time is in the future: sessions record what happened";

/// `times` not after `now` (exactly `now` is fine), else `IN_FUTURE`.
/// `None`s (a field left unchanged, a running session's end) pass.
fn not_after(now: Time, times: &[Option<Time>]) -> Res<()> {
    match times.iter().flatten().any(|&t| t > now) {
        true => Err(IN_FUTURE.into()),
        false => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::{IN_FUTURE, not_after};
    use crate::{
        model::sessions::{Session, SessionError, SessionPatch, SessionQuery, SessionStore},
        test_util::{at, core},
    };

    #[tokio::test]
    async fn add_session_records_on_the_task() {
        let (_tmp, core) = core().await; // root: [a, ws: [b]]

        let s = core
            .add_session(&[1, 0], at(9, 0), at(10, 0), at(20, 0))
            .await
            .unwrap();

        assert_eq!(s.task.name, "b");
        assert_eq!((s.start, s.end), (at(9, 0), Some(at(10, 0))));
    }

    #[tokio::test]
    async fn add_session_on_a_container_is_refused() {
        let (_tmp, core) = core().await;

        let err = core
            .add_session(&[1], at(9, 0), at(10, 0), at(20, 0))
            .await
            .unwrap_err();

        assert!(err.to_string().contains("not a task"), "{err}");
    }

    #[tokio::test]
    async fn add_session_on_the_root_is_refused() {
        let (_tmp, core) = core().await;

        assert!(
            core.add_session(&[], at(9, 0), at(10, 0), at(20, 0))
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn edit_session_moves_the_start() {
        let (_tmp, core) = core().await;
        let s = core
            .add_session(&[0], at(9, 0), at(10, 0), at(20, 0))
            .await
            .unwrap();

        let patch = SessionPatch {
            start: Some(at(8, 30)),
            ..Default::default()
        };
        core.edit_session(s.id, patch, at(20, 0)).await.unwrap();

        let all = core
            .sessions()
            .query(&SessionQuery::default())
            .await
            .unwrap();
        assert_eq!(all[0].start, at(8, 30));
    }

    #[tokio::test]
    async fn split_session_gives_two_pieces() {
        let (_tmp, core) = core().await;
        let s = core
            .add_session(&[0], at(9, 0), at(11, 0), at(20, 0))
            .await
            .unwrap();

        let (a, b) = core.split_session(s.id, at(10, 0)).await.unwrap().unwrap();

        assert_eq!((a.end, b.start), (Some(at(10, 0)), at(10, 0)));
    }

    #[tokio::test]
    async fn cut_session_removes_the_middle() {
        let (_tmp, core) = core().await;
        let s = core
            .add_session(&[0], at(9, 0), at(12, 0), at(20, 0))
            .await
            .unwrap();

        let left = core
            .cut_session(s.id, at(10, 0), at(11, 0), at(20, 0))
            .await
            .unwrap();

        assert_eq!(left.len(), 2);
    }

    #[tokio::test]
    async fn delete_session_hides_it() {
        let (_tmp, core) = core().await;
        let s = core
            .add_session(&[0], at(9, 0), at(10, 0), at(20, 0))
            .await
            .unwrap();

        core.delete_session(s.id).await.unwrap();

        let all = core
            .sessions()
            .query(&SessionQuery::default())
            .await
            .unwrap();
        assert!(all.is_empty());
    }

    #[tokio::test]
    async fn store_refusals_pass_through() {
        let (_tmp, core) = core().await;
        core.add_session(&[0], at(9, 0), at(10, 0), at(20, 0))
            .await
            .unwrap();

        let err = core
            .add_session(&[0], at(9, 30), at(10, 30), at(20, 0))
            .await
            .unwrap_err();

        assert_eq!(err.to_string(), SessionError::Overlap.to_string());
    }

    // ---------- not in the future (now: 12:00) ----------

    const NOW: fn() -> crate::model::time::Time = || at(12, 0);

    /// `a` with one session 9:00-10:00; returns it with the core.
    async fn with_session() -> (tempfile::TempDir, crate::core::Core, Session) {
        let (tmp, core) = core().await;
        let s = core
            .add_session(&[0], at(9, 0), at(10, 0), at(20, 0))
            .await
            .unwrap();
        (tmp, core, s)
    }

    /// The only session, read back through the store.
    async fn the_session(core: &crate::core::Core) -> Session {
        let all = core.sessions_of(&[0]).await.unwrap();
        assert_eq!(all.len(), 1, "{all:?}");
        all[0].clone()
    }

    #[tokio::test]
    async fn edit_to_a_future_end_is_refused_and_changes_nothing() {
        let (_tmp, core, s) = with_session().await;
        let patch = SessionPatch {
            end: Some(at(12, 1)),
            ..Default::default()
        };

        let err = core.edit_session(s.id, patch, NOW()).await.unwrap_err();

        assert_eq!(err.to_string(), IN_FUTURE);
        assert_eq!(the_session(&core).await, s); // checked before the store
    }

    #[tokio::test]
    async fn edit_to_a_future_start_is_refused() {
        let (_tmp, core, s) = with_session().await;
        let patch = SessionPatch {
            start: Some(at(13, 0)),
            ..Default::default()
        };

        let err = core.edit_session(s.id, patch, NOW()).await.unwrap_err();

        assert_eq!(err.to_string(), IN_FUTURE);
        assert_eq!(the_session(&core).await, s);
    }

    #[tokio::test]
    async fn edit_to_exactly_now_is_allowed() {
        let (_tmp, core, s) = with_session().await;
        let patch = SessionPatch {
            end: Some(NOW()),
            ..Default::default()
        };

        core.edit_session(s.id, patch, NOW()).await.unwrap();

        assert_eq!(the_session(&core).await.end, Some(NOW()));
    }

    /// Nothing to check: whatever happens then is the store's business.
    #[tokio::test]
    async fn a_patch_without_times_passes_the_check() {
        let (_tmp, core, s) = with_session().await;

        let result = core
            .edit_session(s.id, SessionPatch::default(), NOW())
            .await;

        if let Err(e) = result {
            assert_ne!(e.to_string(), IN_FUTURE);
        }
    }

    #[tokio::test]
    async fn cut_to_the_future_is_refused_and_changes_nothing() {
        let (_tmp, core, s) = with_session().await;

        let err = core
            .cut_session(s.id, at(9, 30), at(12, 30), NOW())
            .await
            .unwrap_err();

        assert_eq!(err.to_string(), IN_FUTURE);
        assert_eq!(the_session(&core).await, s);
    }

    #[test]
    fn not_after_checks_every_given_time() {
        assert!(not_after(NOW(), &[]).is_ok());
        assert!(not_after(NOW(), &[None, Some(at(11, 0)), Some(NOW())]).is_ok());
        assert!(not_after(NOW(), &[Some(at(11, 0)), Some(at(12, 1))]).is_err());
    }
}
