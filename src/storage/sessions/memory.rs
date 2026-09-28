use std::sync::Mutex;

use super::rules::{Cut, Split, check_span, intersects, plan_cut, plan_edit, plan_split};
use crate::model::{
    sessions::{
        Session, SessionError, SessionId, SessionPatch, SessionQuery, SessionSource, SessionStore,
        TaskRef,
    },
    time::{Clock, Time},
};

/// Reference backend: everything in a `Vec`. Used by the contract tests.
pub struct MemorySessions {
    inner: Mutex<Inner>,
}

struct Inner {
    sessions: Vec<Session>,
    clock: Clock,
}

impl MemorySessions {
    pub fn new(clock: Clock) -> Self {
        Self {
            inner: Mutex::new(Inner {
                sessions: Vec::new(),
                clock,
            }),
        }
    }
}

impl Inner {
    fn now(&self) -> Time {
        (self.clock)()
    }

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
            edited_at: None,
            deleted_at: None,
            created_at: self.now(),
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
        let now = self.now();
        let Some(i) = self.running_index() else {
            return Ok(None);
        };
        let session = &mut self.sessions[i];
        if at < session.start {
            session.deleted_at = Some(now); // a clock error, not an edit
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
            .filter(|s| intersects(s.start, s.end, q.from, q.to))
            .cloned()
            .collect();
        found.sort_by_key(|s| s.start);
        found
    }

    fn add(&mut self, task: TaskRef, start: Time, end: Time) -> Result<Session, SessionError> {
        check_span(start, end)?;
        if self.overlaps(start, Some(end), None) {
            return Err(SessionError::Overlap);
        }
        let session = Session {
            id: SessionId::new(),
            task,
            start,
            end: Some(end),
            source: SessionSource::Manual,
            created_at: self.now(),
            edited_at: None, // manual adds have no edit entry
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
            .any(|s| intersects(s.start, s.end, Some(start), end))
    }

    fn edit(&mut self, id: SessionId, patch: SessionPatch) -> Result<(), SessionError> {
        let now = self.now();
        let i = self.index(id)?;
        let old = &self.sessions[i];
        let (start, end) = plan_edit(old.start, old.end, &patch)?;
        if self.overlaps(start, end, Some(id)) {
            return Err(SessionError::Overlap);
        }
        let session = &mut self.sessions[i];
        session.start = start;
        session.end = end;
        session.edited_at = Some(now);
        Ok(())
    }

    fn split(
        &mut self,
        id: SessionId,
        at: Time,
    ) -> Result<Option<(Session, Session)>, SessionError> {
        let now = self.now();
        let i = self.index(id)?;
        let session = &mut self.sessions[i];
        let Some(Split { at, end }) = plan_split(session.start, session.end, at)? else {
            return Ok(None); // on an edge: nothing changes
        };
        session.end = Some(at);
        session.edited_at = Some(now);
        let first = session.clone(); // the second piece inherits `created_at`
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
        let now = self.now();
        let i = self.index(id)?;
        let session = &mut self.sessions[i];
        let plan = plan_cut(session.start, session.end, from, to)?;
        session.edited_at = Some(now);
        match plan {
            Cut::TrimStart(start) => {
                session.start = start;
                Ok(vec![session.clone()])
            }
            Cut::TrimEnd(end) => {
                session.end = Some(end);
                Ok(vec![session.clone()])
            }
            Cut::Gap { from, to } => {
                // the second piece takes the old end: a running session keeps running
                let second = Session {
                    id: SessionId::new(),
                    start: to,
                    ..session.clone()
                };
                session.end = Some(from);
                let first = session.clone();
                self.sessions.push(second.clone());
                Ok(vec![first, second])
            }
        }
    }

    fn delete(&mut self, id: SessionId) -> Result<(), SessionError> {
        let now = self.now(); // first: `self.now()` borrows all of `self`
        let i = self.index(id)?;
        let session = &mut self.sessions[i];
        if session.end.is_none() && now >= session.start {
            session.end = Some(now); // running: stopped first
        }
        session.deleted_at = Some(now);
        session.edited_at = Some(now); // a delete is an edit entry too
        Ok(())
    }
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

    async fn delete(&self, id: SessionId) -> Result<(), SessionError> {
        self.inner.lock().unwrap().delete(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    super::super::contract::store_contract!(MemorySessions::new);
}
