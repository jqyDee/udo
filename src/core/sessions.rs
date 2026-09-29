//! Correcting recorded time: add, edit, split, cut and delete sessions.
//! Thin: the store keeps its own rules (overlap, outside, running).

use crate::{
    Res,
    core::Core,
    model::{
        sessions::{Session, SessionId, SessionPatch, SessionStore},
        time::Time,
    },
};

impl Core {
    /// Record a session on the task at `path` by hand (source `manual`).
    pub async fn add_session(&self, path: &[usize], start: Time, end: Time) -> Res<Session> {
        let task = self.task_ref(path)?;
        Ok(self.storage.sessions.add(task, start, end).await?)
    }

    /// Move a session's start and / or end.
    pub async fn edit_session(&self, id: SessionId, patch: SessionPatch) -> Res<()> {
        Ok(self.storage.sessions.edit(id, patch).await?)
    }

    /// Split a session in two at `at`; `at` on its start or end: `None`.
    pub async fn split_session(&self, id: SessionId, at: Time) -> Res<Option<(Session, Session)>> {
        Ok(self.storage.sessions.split(id, at).await?)
    }

    /// Remove `[from, to)` from a session; returns what is left.
    pub async fn cut_session(&self, id: SessionId, from: Time, to: Time) -> Res<Vec<Session>> {
        Ok(self.storage.sessions.cut(id, from, to).await?)
    }

    /// Soft delete: hidden, the data stays.
    pub async fn delete_session(&self, id: SessionId) -> Res<()> {
        Ok(self.storage.sessions.delete(id).await?)
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        model::sessions::{SessionError, SessionPatch, SessionQuery, SessionStore},
        test_util::{at, core},
    };

    #[tokio::test]
    async fn add_session_records_on_the_task() {
        let (_tmp, core) = core().await; // root: [a, ws: [b]]

        let s = core
            .add_session(&[1, 0], at(9, 0), at(10, 0))
            .await
            .unwrap();

        assert_eq!(s.task.name, "b");
        assert_eq!((s.start, s.end), (at(9, 0), Some(at(10, 0))));
    }

    #[tokio::test]
    async fn add_session_on_a_container_is_refused() {
        let (_tmp, core) = core().await;

        let err = core
            .add_session(&[1], at(9, 0), at(10, 0))
            .await
            .unwrap_err();

        assert!(err.to_string().contains("not a task"), "{err}");
    }

    #[tokio::test]
    async fn add_session_on_the_root_is_refused() {
        let (_tmp, core) = core().await;

        assert!(core.add_session(&[], at(9, 0), at(10, 0)).await.is_err());
    }

    #[tokio::test]
    async fn edit_session_moves_the_start() {
        let (_tmp, core) = core().await;
        let s = core.add_session(&[0], at(9, 0), at(10, 0)).await.unwrap();

        let patch = SessionPatch {
            start: Some(at(8, 30)),
            ..Default::default()
        };
        core.edit_session(s.id, patch).await.unwrap();

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
        let s = core.add_session(&[0], at(9, 0), at(11, 0)).await.unwrap();

        let (a, b) = core.split_session(s.id, at(10, 0)).await.unwrap().unwrap();

        assert_eq!((a.end, b.start), (Some(at(10, 0)), at(10, 0)));
    }

    #[tokio::test]
    async fn cut_session_removes_the_middle() {
        let (_tmp, core) = core().await;
        let s = core.add_session(&[0], at(9, 0), at(12, 0)).await.unwrap();

        let left = core.cut_session(s.id, at(10, 0), at(11, 0)).await.unwrap();

        assert_eq!(left.len(), 2);
    }

    #[tokio::test]
    async fn delete_session_hides_it() {
        let (_tmp, core) = core().await;
        let s = core.add_session(&[0], at(9, 0), at(10, 0)).await.unwrap();

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
        core.add_session(&[0], at(9, 0), at(10, 0)).await.unwrap();

        let err = core
            .add_session(&[0], at(9, 30), at(10, 30))
            .await
            .unwrap_err();

        assert_eq!(err.to_string(), SessionError::Overlap.to_string());
    }
}
