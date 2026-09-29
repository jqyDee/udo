use crate::model::{
    id::NodeId,
    time::{Minutes, Time},
};

mod error;
mod id;
mod source;
mod store;
mod summary;
mod task_ref;

pub use crate::model::sessions::{
    error::SessionError, id::SessionId, source::SessionSource, store::SessionStore,
    summary::TimeSummary, task_ref::TaskRef,
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

impl Session {
    /// How long it ran: until its end, or until `now` while running. Whole
    /// minutes, rounded down; never negative (a clock behind the start: 0).
    pub fn duration(&self, now: Time) -> Minutes {
        let end = self.end.unwrap_or(now);
        let minutes = (end - self.start).num_minutes().max(0);
        Minutes::new(u32::try_from(minutes).unwrap_or(u32::MAX))
    }
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

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use chrono::TimeDelta;

    use super::*;
    use crate::test_util::at;

    fn session(start: Time, end: Option<Time>) -> Session {
        Session {
            id: SessionId::new(),
            task: TaskRef {
                id: NodeId::new(),
                name: "lab 3".into(),
                description: String::new(),
                container_dir: PathBuf::from("/uni"),
                container_id: NodeId::new(),
            },
            start,
            end,
            source: SessionSource::Manual,
            created_at: start,
            edited_at: None,
            deleted_at: None,
        }
    }

    #[test]
    fn an_ended_session_runs_until_its_end() {
        let s = session(at(14, 0), Some(at(15, 12)));

        assert_eq!(s.duration(at(20, 0)), Minutes::new(72)); // `now` is ignored
    }

    #[test]
    fn a_running_session_runs_until_now() {
        let s = session(at(14, 0), None);

        assert_eq!(s.duration(at(14, 45)), Minutes::new(45));
    }

    #[test]
    fn part_of_a_minute_is_rounded_down() {
        let s = session(at(14, 0), None);

        assert_eq!(s.duration(at(14, 0) + TimeDelta::seconds(59)), Minutes::new(0));
    }

    #[test]
    fn a_clock_behind_the_start_gives_zero() {
        let s = session(at(14, 0), None);

        assert_eq!(s.duration(at(13, 0)), Minutes::new(0));
    }
}
