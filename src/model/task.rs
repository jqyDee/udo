use std::{fmt, path::PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::model::id::NodeId;

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct Task {
    pub id: NodeId,
    pub name: String,
    pub dir: Option<PathBuf>,
    pub status: TaskStatus,
    pub due_date: DateTime<Utc>,
}

impl Task {
    pub fn new(name: String, dir: Option<PathBuf>, due_date: DateTime<Utc>) -> Self {
        Self {
            id: NodeId::new(),
            name,
            dir,
            status: TaskStatus::Pending,
            due_date,
        }
    }

    pub fn update(&mut self, patch: TaskPatch) {
        if let Some(name) = patch.name {
            self.name = name;
        }
        if let Some(status) = patch.status {
            self.status = status;
        }
        if let Some(due_date) = patch.due_date {
            self.due_date = due_date;
        }
        if let Some(dir) = patch.dir {
            self.dir = Some(dir);
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub enum TaskStatus {
    Pending,
    /// When not Pending and not in progress
    Stale,
    InProgress,
    Finished,
}

/// Words for the UI, same as the key help ("mark to do", "mark done", ...).
/// `pad`, so width/alignment like `{:<12}` work.
impl fmt::Display for TaskStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.pad(match self {
            Self::Pending => "to do",
            Self::Stale => "stale",
            Self::InProgress => "in progress",
            Self::Finished => "done",
        })
    }
}

#[derive(Default)]
pub struct TaskPatch {
    pub name: Option<String>,
    pub dir: Option<PathBuf>,
    pub status: Option<TaskStatus>,
    pub due_date: Option<DateTime<Utc>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_task_is_pending() {
        let t = Task::new("t".into(), None, Utc::now());
        assert_eq!(t.status, TaskStatus::Pending);
        assert!(t.dir.is_none());
    }

    #[test]
    fn update_changes_only_given_fields() {
        let due = Utc::now();
        let mut t = Task::new("t".into(), Some("/tmp/t".into()), due);

        t.update(TaskPatch {
            status: Some(TaskStatus::InProgress),
            ..Default::default()
        });

        assert_eq!(t.status, TaskStatus::InProgress);
        assert_eq!(t.name, "t");
        assert_eq!(t.dir, Some(PathBuf::from("/tmp/t")));
        assert_eq!(t.due_date, due);
    }

    #[test]
    fn update_sets_all_fields() {
        let mut t = Task::new("t".into(), None, Utc::now());
        let due = Utc::now();

        t.update(TaskPatch {
            name: Some("new".into()),
            dir: Some("/tmp/new".into()),
            status: Some(TaskStatus::Finished),
            due_date: Some(due),
        });

        assert_eq!(t.name, "new");
        assert_eq!(t.dir, Some(PathBuf::from("/tmp/new")));
        assert_eq!(t.status, TaskStatus::Finished);
        assert_eq!(t.due_date, due);
    }

    #[test]
    fn status_display_matches_key_help_and_pads() {
        assert_eq!(TaskStatus::Pending.to_string(), "to do");
        assert_eq!(TaskStatus::InProgress.to_string(), "in progress");
        assert_eq!(TaskStatus::Stale.to_string(), "stale");
        assert_eq!(TaskStatus::Finished.to_string(), "done");
        assert_eq!(format!("[{:<7}]", TaskStatus::Finished), "[done   ]");
    }
}
