use std::fmt;

use uuid::Uuid;

use crate::model::{id::NodeId, time::Time};

/// Stable ID of a work session. v7 like `NodeId`: time-ordered, and safe to
/// copy between backends.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SessionId(Uuid);

impl SessionId {
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        Self(Uuid::now_v7())
    }
}

/// Where a session was recorded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionSource {
    Manual,
}

/// What a session remembers about its task; survives a task delete.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskRef {
    pub id: NodeId,
    pub name: String,
    pub description: String,
    pub container_path: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Session {
    pub id: SessionId,
    pub task: TaskRef,
    /// Each with its own offset (the wall clock at start / at end).
    pub start: Time,
    /// `None` = running.
    pub end: Option<Time>,
    pub source: SessionSource,
    /// Has entries in the edit log. Computed, never stored.
    pub edited: bool,
    pub deleted_at: Option<Time>,
}

/// A correction. `None` = keep.
#[derive(Debug, Clone, Default)]
pub struct SessionPatch {
    pub start: Option<Time>,
    pub end: Option<Time>,
}

/// Filters for `query`. Use `..Default::default()` at call sites. Results
/// are sorted by start, so every backend returns the same order.
#[derive(Debug, Clone, Default)]
pub struct SessionQuery {
    /// `None` = all tasks.
    pub tasks: Option<Vec<NodeId>>,
    /// Sessions that intersect `[from, to)`.
    pub from: Option<Time>,
    pub to: Option<Time>,
    pub include_deleted: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionError {
    NotFound,
    EndBeforeStart,
    Overlap,
    AlreadyRunning,
    /// `split` / `cut` time not inside the session.
    OutsideSession,
    /// `cut` would remove everything (use `delete`).
    WholeSession,
    Running,
    /// Database error, as text (keeps the enum `Send + Sync + PartialEq`).
    Backend(String),
}

impl fmt::Display for SessionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound => write!(f, "session not found"),
            Self::EndBeforeStart => write!(f, "end must be after start"),
            Self::Overlap => write!(f, "overlaps another session"),
            Self::AlreadyRunning => write!(f, "another session is already running"),
            Self::OutsideSession => write!(f, "time outside of session time"),
            Self::WholeSession => write!(f, "would remove the whole session, delete it instead"),
            Self::Running => write!(f, "session is still running, stop it first"),
            Self::Backend(e) => write!(f, "session storage: {e}"),
        }
    }
}

impl std::error::Error for SessionError {}

/// Where work sessions live. Every backend keeps these rules (tested by
/// `storage::sessions::contract`). `edit`, `split`, `cut`, `delete`: an
/// unknown or deleted id is `NotFound`.
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
    /// `EndBeforeStart`, and the session is soft-deleted.
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
    /// Soft delete at `at`: hidden, never removed. A running session is
    /// stopped at `at` first.
    async fn delete(&self, id: SessionId, at: Time) -> Result<(), SessionError>;
}
