use std::{fmt, path::PathBuf};

use serde::{Deserialize, Serialize};

use crate::model::time::Time;

/// Task body of a `Node` (id and name live on the node).
///
/// Only "done" is stored (`done_at`); the status is computed from it and
/// the task's sessions (`status`), "overdue" from the due date
/// (`is_overdue`).
#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct Task {
    pub dir: Option<PathBuf>,
    /// When the task was marked done; `None`: not done.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub done_at: Option<Time>,
    pub due_date: Time,
}

impl Task {
    pub fn new(dir: Option<PathBuf>, due_date: Time) -> Self {
        Self {
            dir,
            done_at: None,
            due_date,
        }
    }

    /// The status, from `done_at` and whether the task has (not removed)
    /// sessions. Every view asks this.
    pub fn status(&self, has_sessions: bool) -> TaskStatus {
        match (self.done_at, has_sessions) {
            (Some(_), _) => TaskStatus::Done,
            (None, true) => TaskStatus::Started,
            (None, false) => TaskStatus::ToDo,
        }
    }

    /// Not done and the due date has passed (exactly `now`: not yet).
    pub fn is_overdue(&self, now: Time) -> bool {
        self.done_at.is_none() && self.due_date < now
    }

    pub fn update(&mut self, patch: TaskPatch) {
        if let Some(done_at) = patch.done_at {
            self.done_at = done_at;
        }
        if let Some(due_date) = patch.due_date {
            self.due_date = due_date;
        }
        if let Some(dir) = patch.dir {
            self.dir = Some(dir);
        }
    }
}

/// Computed, never stored: see `Task::status`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskStatus {
    ToDo,
    /// Time was recorded (not: work goes on right now, that is the timer).
    Started,
    Done,
}

/// Words for the UI. `pad`, so width/alignment like `{:<12}` work.
impl fmt::Display for TaskStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.pad(match self {
            Self::ToDo => "to do",
            Self::Started => "started",
            Self::Done => "done",
        })
    }
}

#[derive(Default)]
pub struct TaskPatch {
    pub dir: Option<PathBuf>,
    /// `Some(None)`: reopen.
    pub done_at: Option<Option<Time>>,
    pub due_date: Option<Time>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{model::time, test_util::at};

    #[test]
    fn new_task_is_to_do() {
        let t = Task::new(None, time::now());
        assert_eq!(t.status(false), TaskStatus::ToDo);
        assert!(t.dir.is_none());
    }

    #[test]
    fn done_wins_over_sessions() {
        let mut t = Task::new(None, at(22, 0));
        t.done_at = Some(at(14, 0));

        assert_eq!(t.status(true), TaskStatus::Done);
        assert_eq!(t.status(false), TaskStatus::Done);
    }

    #[test]
    fn sessions_make_it_started() {
        let t = Task::new(None, at(22, 0));

        assert_eq!(t.status(true), TaskStatus::Started);
        assert_eq!(t.status(false), TaskStatus::ToDo);
    }

    #[test]
    fn overdue_when_past_due_and_not_done() {
        let mut t = Task::new(None, at(12, 0));

        assert!(t.is_overdue(at(12, 1)));
        assert!(!t.is_overdue(at(12, 0))); // exactly now: not yet
        assert!(!t.is_overdue(at(11, 0))); // due in the future

        t.done_at = Some(at(13, 0));
        assert!(!t.is_overdue(at(14, 0))); // done: never overdue
    }

    #[test]
    fn update_changes_only_given_fields() {
        let due = time::now();
        let mut t = Task::new(Some("/tmp/t".into()), due);

        t.update(TaskPatch {
            done_at: Some(Some(at(14, 0))),
            ..Default::default()
        });

        assert_eq!(t.done_at, Some(at(14, 0)));
        assert_eq!(t.dir, Some(PathBuf::from("/tmp/t")));
        assert_eq!(t.due_date, due);
    }

    #[test]
    fn update_sets_all_fields() {
        let mut t = Task::new(None, time::now());
        t.done_at = Some(at(9, 0));
        let due = time::now();

        t.update(TaskPatch {
            dir: Some("/tmp/new".into()),
            done_at: Some(None),
            due_date: Some(due),
        });

        assert_eq!(t.dir, Some(PathBuf::from("/tmp/new")));
        assert_eq!(t.done_at, None);
        assert_eq!(t.due_date, due);
    }

    #[test]
    fn status_display_pads() {
        assert_eq!(TaskStatus::ToDo.to_string(), "to do");
        assert_eq!(TaskStatus::Started.to_string(), "started");
        assert_eq!(TaskStatus::Done.to_string(), "done");
        assert_eq!(format!("[{:<7}]", TaskStatus::Done), "[done   ]");
    }
}
