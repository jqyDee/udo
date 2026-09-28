use crate::model::{
    sessions::{
        Session, SessionError, SessionId, SessionPatch, SessionQuery, SessionSource, TaskRef,
    },
    time::Time,
};

/// Where work sessions live. Every backend keeps these rules (tested by
/// `storage::sessions::contract`). `edit`, `split`, `cut`, `delete`: an
/// unknown or deleted id is `NotFound`.
///
/// Event times (start, end, split / cut points) come from the caller;
/// bookkeeping times (`created_at`, `edited_at`, `deleted_at`) from the
/// store's `Clock`, passed when the store is built.
#[allow(async_fn_in_trait)] // no Send bound needed: dispatch goes through an enum, not dyn
pub trait SessionStore {
    /// Start timing; a running session is stopped first (one timer).
    /// Same task already running: no-op, returns the running session.
    async fn start(
        &self,
        task: TaskRef,
        source: SessionSource,
        at: Time,
    ) -> Result<Session, SessionError>;
    /// Stop the running session, if any. `at` before its start:
    /// `EndBeforeStart`, and the session is soft-deleted (a clock error,
    /// not a correction: `edited_at` stays unset).
    async fn stop(&self, at: Time) -> Result<Option<Session>, SessionError>;
    /// The session running right now, if any (at most one: one timer).
    async fn running(&self) -> Result<Option<Session>, SessionError>;
    /// Sessions matching `q`, sorted by start.
    async fn query(&self, q: &SessionQuery) -> Result<Vec<Session>, SessionError>;
    /// A manual session (source `manual`). No edit entry: counts as unedited.
    /// End not after start: `EndBeforeStart`. Over another session: `Overlap`.
    async fn add(&self, task: TaskRef, start: Time, end: Time) -> Result<Session, SessionError>;
    /// Split into two sessions at `at` (logged as an edit).
    /// `at` outside the session: `OutsideSession`. `at` on its start or end:
    /// no-op, `None`. Running session: `Running`.
    async fn split(
        &self,
        id: SessionId,
        at: Time,
    ) -> Result<Option<(Session, Session)>, SessionError>;
    /// Remove `[from, to)` from the session (e.g. a lunch break the timer
    /// ran through). Returns what is left, sorted by start: two pieces (cut
    /// inside; the first keeps the id) or one (cut over start or end). Logged
    /// as one edit; the removed part is not kept (the log has the old times).
    /// Clamped to the session; no overlap at all: `OutsideSession`. Covers
    /// everything: `WholeSession` (use `delete`). `to` not after `from`:
    /// `EndBeforeStart`. Running sessions are fine: the last piece keeps
    /// running. The caller keeps `to` in the past.
    async fn cut(&self, id: SessionId, from: Time, to: Time) -> Result<Vec<Session>, SessionError>;
    /// Correction: move start / end. Logged in `session_edits`. Setting the
    /// end of a running session: `Running` (use `stop`).
    async fn edit(&self, id: SessionId, patch: SessionPatch) -> Result<(), SessionError>;
    /// Soft delete: hidden, never removed. A running session is stopped
    /// (at the clock's now) first.
    async fn delete(&self, id: SessionId) -> Result<(), SessionError>;
}
