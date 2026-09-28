use crate::model::{id::NodeId, time::Time};

mod error;
mod id;
mod source;
mod store;
mod task_ref;

pub use crate::model::sessions::{
    error::SessionError, id::SessionId, source::SessionSource, store::SessionStore,
    task_ref::TaskRef,
};

#[derive(Debug, Clone, PartialEq)]
pub struct Session {
    pub id: SessionId,
    pub task: TaskRef,
    /// Each with its own offset (the wall clock at start / at end).
    pub start: Time,
    /// `None` = running.
    pub end: Option<Time>,
    pub source: SessionSource,
    /// When it was recorded (store clock). Split / cut pieces inherit it.
    pub created_at: Time,
    /// Last correction (store clock); `None` = never edited.
    pub edited_at: Option<Time>,
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
