use std::sync::Mutex;

use crate::model::{
    sessions::{
        Session, SessionError, SessionId, SessionPatch, SessionQuery, SessionSource, SessionStore,
        TaskRef,
    },
    time::Time,
};

/// Reference backend: everything in a `Vec`. Used by the contract tests.
#[derive(Default)]
pub struct MemorySessions {
    inner: Mutex<Inner>,
}

#[derive(Default)]
struct Inner {
    sessions: Vec<Session>,
}

impl Inner {
    fn start(
        &mut self,
        task: TaskRef,
        source: SessionSource,
        at: Time,
    ) -> Result<Session, SessionError> {
        if let Some(i) = self.running_index()
            && self.sessions[i].task.id == task.id
        {
            return Ok(self.sessions[i].clone());
        }
        self.stop_running(at)?;

        let session = Session {
            id: SessionId::new(),
            task,
            start: at,
            end: None,
            source,
            edited: false,
            deleted_at: None,
        };

        self.sessions.push(session.clone());
        Ok(session)
    }

    fn running(&self) -> Option<Session> {
        self.running_index().map(|i| self.sessions[i].clone())
    }

    /// Stop the running session at `at`, if any. Shared by `start` and
    /// `stop`. `at` before its start: soft-deleted, `EndBeforeStart`.
    fn stop_running(&mut self, at: Time) -> Result<Option<Session>, SessionError> {
        let Some(i) = self.running_index() else {
            return Ok(None);
        };
        let session = &mut self.sessions[i];
        if at < session.start {
            session.deleted_at = Some(at); // the caller's "now"; the store has no clock
            return Err(SessionError::EndBeforeStart);
        }
        session.end = Some(at);
        Ok(Some(session.clone()))
    }

    fn query(&self, q: &SessionQuery) -> Vec<Session> {
        let mut found: Vec<Session> = self
            .sessions
            .iter()
            .filter(|s| q.include_deleted || s.deleted_at.is_none())
            .filter(|s| q.tasks.as_ref().is_none_or(|ids| ids.contains(&s.task.id)))
            .filter(|s| intersects(s, q.from, q.to))
            .cloned()
            .collect();
        found.sort_by_key(|s| s.start);
        found
    }

    fn add(&mut self, task: TaskRef, start: Time, end: Time) -> Result<Session, SessionError> {
        if end <= start {
            return Err(SessionError::EndBeforeStart);
        }
        if self.overlaps(start, Some(end), None) {
            return Err(SessionError::Overlap);
        }
        let session = Session {
            id: SessionId::new(),
            task,
            start,
            end: Some(end),
            source: SessionSource::Manual,
            edited: false,
            deleted_at: None,
        };
        self.sessions.push(session.clone());
        Ok(session)
    }

    /// Index of the running session: no end, not deleted.
    fn running_index(&self) -> Option<usize> {
        self.sessions
            .iter()
            .position(|s| s.end.is_none() && s.deleted_at.is_none())
    }

    /// Index of a visible session; unknown or deleted: `NotFound`.
    fn index(&self, id: SessionId) -> Result<usize, SessionError> {
        self.sessions
            .iter()
            .position(|s| s.id == id && s.deleted_at.is_none())
            .ok_or(SessionError::NotFound)
    }

    /// Does `[start, end)` overlap a visible session other than `except`?
    /// `end: None` = open (running). Half-open: 14–15 and 15–16 touch, no overlap.
    fn overlaps(&self, start: Time, end: Option<Time>, except: Option<SessionId>) -> bool {
        self.sessions
            .iter()
            .filter(|s| s.deleted_at.is_none() && Some(s.id) != except)
            .any(|s| intersects(s, Some(start), end))
    }

    fn edit(&mut self, id: SessionId, patch: SessionPatch) -> Result<(), SessionError> {
        let i = self.index(id)?;
        let old = &self.sessions[i];
        if old.end.is_none() && patch.end.is_some() {
            return Err(SessionError::Running); // use `stop`
        }
        let start = patch.start.unwrap_or(old.start);
        let end = patch.end.or(old.end);
        if end.is_some_and(|e| e <= start) {
            return Err(SessionError::EndBeforeStart);
        }
        if self.overlaps(start, end, Some(id)) {
            return Err(SessionError::Overlap);
        }
        let session = &mut self.sessions[i];
        session.start = start;
        session.end = end;
        session.edited = true;
        Ok(())
    }

    fn split(
        &mut self,
        id: SessionId,
        at: Time,
    ) -> Result<Option<(Session, Session)>, SessionError> {
        let i = self.index(id)?;
        let session = &mut self.sessions[i];
        let Some(end) = session.end else {
            return Err(SessionError::Running);
        };
        if at < session.start || at > end {
            return Err(SessionError::OutsideSession);
        }
        if at == session.start || at == end {
            return Ok(None); // would give a 0-minute half
        }
        session.end = Some(at);
        session.edited = true;
        let first = session.clone();
        let second = Session {
            id: SessionId::new(),
            start: at,
            end: Some(end),
            ..first.clone()
        };
        self.sessions.push(second.clone());
        Ok(Some((first, second)))
    }

    fn cut(&mut self, id: SessionId, from: Time, to: Time) -> Result<Vec<Session>, SessionError> {
        let i = self.index(id)?;
        if to <= from {
            return Err(SessionError::EndBeforeStart);
        }
        let session = &mut self.sessions[i];
        let (start, end) = (session.start, session.end);
        // clamp to the session; a running one has no end to clamp to
        let from = from.max(start);
        let to = end.map_or(to, |end| to.min(end));
        if from >= to {
            return Err(SessionError::OutsideSession);
        }
        if from == start && Some(to) == end {
            return Err(SessionError::WholeSession);
        }
        session.edited = true;
        if from == start {
            session.start = to; // cut over the start
            return Ok(vec![session.clone()]);
        }
        if Some(to) == end {
            session.end = Some(from); // cut over the end
            return Ok(vec![session.clone()]);
        }
        // cut inside: a gap; a running session keeps running in the second piece
        session.end = Some(from);
        let first = session.clone();
        let second = Session {
            id: SessionId::new(),
            start: to,
            end,
            ..first.clone()
        };
        self.sessions.push(second.clone());
        Ok(vec![first, second])
    }

    fn delete(&mut self, id: SessionId, at: Time) -> Result<(), SessionError> {
        let i = self.index(id)?;
        let session = &mut self.sessions[i];
        if session.end.is_none() && at >= session.start {
            session.end = Some(at); // running: stopped first
        }
        session.deleted_at = Some(at);
        session.edited = true; // a delete is an edit entry too
        Ok(())
    }
}

/// Does `s` intersect `[from, to)`? `None` = unbounded on that side; a
/// running session counts as open-ended.
fn intersects(s: &Session, from: Option<Time>, to: Option<Time>) -> bool {
    let starts_before_to = to.is_none_or(|to| s.start < to);
    let ends_after_from = from.is_none_or(|from| s.end.is_none_or(|end| from < end));
    starts_before_to && ends_after_from
}

impl SessionStore for MemorySessions {
    async fn start(
        &self,
        task: TaskRef,
        source: SessionSource,
        at: Time,
    ) -> Result<Session, SessionError> {
        self.inner.lock().unwrap().start(task, source, at)
    }

    async fn stop(&self, at: Time) -> Result<Option<Session>, SessionError> {
        self.inner.lock().unwrap().stop_running(at)
    }

    async fn running(&self) -> Result<Option<Session>, SessionError> {
        Ok(self.inner.lock().unwrap().running())
    }

    async fn query(&self, q: &SessionQuery) -> Result<Vec<Session>, SessionError> {
        Ok(self.inner.lock().unwrap().query(q))
    }

    async fn add(&self, task: TaskRef, start: Time, end: Time) -> Result<Session, SessionError> {
        self.inner.lock().unwrap().add(task, start, end)
    }

    async fn split(
        &self,
        id: SessionId,
        at: Time,
    ) -> Result<Option<(Session, Session)>, SessionError> {
        self.inner.lock().unwrap().split(id, at)
    }

    async fn cut(&self, id: SessionId, from: Time, to: Time) -> Result<Vec<Session>, SessionError> {
        self.inner.lock().unwrap().cut(id, from, to)
    }

    async fn edit(&self, id: SessionId, patch: SessionPatch) -> Result<(), SessionError> {
        self.inner.lock().unwrap().edit(id, patch)
    }

    async fn delete(&self, id: SessionId, at: Time) -> Result<(), SessionError> {
        self.inner.lock().unwrap().delete(id, at)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    super::super::contract::store_contract!(MemorySessions::default);
}
