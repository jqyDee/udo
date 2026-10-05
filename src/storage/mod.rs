use std::path::Path;

use crate::{
    Res,
    model::time::now,
    storage::{
        estimates::{Estimates, memory::MemoryEstimates, sqlite::SqliteEstimates},
        sessions::{Sessions, memory::MemorySessions, sqlite::SqliteSessions},
    },
};

mod dispatch;
mod edit_kind;
pub mod estimates;
pub mod sessions;
mod sql;
pub mod sqlite;
mod time;

/// Every store udo uses, opened once at startup (sessions now, the tree
/// later).
pub struct Storage {
    pub sessions: Sessions,
    pub estimates: Estimates,
}

impl Storage {
    /// Open (or create) `DB_FILE_NAME` in the root dir.
    pub fn open_db(root_dir: &Path) -> Res<Self> {
        let path = root_dir.join(sqlite::DB_FILE_NAME);
        Ok(Self {
            sessions: Sessions::Sqlite(SqliteSessions::new(sqlite::open(&path)?, now)),
            estimates: Estimates::Sqlite(SqliteEstimates::new(sqlite::open(&path)?, now)),
        })
    }

    /// Nothing on disk (tests).
    pub fn in_memory() -> Self {
        Self {
            sessions: Sessions::Memory(MemorySessions::new(now)),
            estimates: Estimates::Memory(MemoryEstimates::new(now)),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        model::{
            estimate_store::{EstimateStore, NewEstimate, Reason},
            id::NodeId,
            sessions::{SessionQuery, SessionStore, TaskRef},
            time::Minutes,
        },
        test_util::at,
    };

    use super::*;

    #[test]
    fn open_db_creates_the_file_in_the_root() {
        let dir = tempfile::tempdir().unwrap();

        Storage::open_db(dir.path()).unwrap();

        assert!(dir.path().join(sqlite::DB_FILE_NAME).exists());
    }

    /// Sessions and estimates use one file, each with its own connection
    /// (WAL): what both write is there when opened again.
    #[tokio::test]
    async fn sessions_and_estimates_share_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let task = TaskRef {
            id: NodeId::new(),
            name: "lab 3".into(),
            description: String::new(),
            container_id: NodeId::new(),
            container_dir: "uni/cs".into(),
        };
        let estimate = NewEstimate {
            task: task.id,
            minutes: Minutes::new(90),
            method: "prior".into(),
            version: 1,
            done_tasks: 0,
            open_tasks: 0,
            prior: Some(Minutes::new(90)),
            reason: Reason::Created,
        };
        {
            let storage = Storage::open_db(dir.path()).unwrap();
            storage
                .sessions
                .add(task.clone(), at(9, 0), at(10, 0))
                .await
                .unwrap();
            storage
                .estimates
                .record(estimate.clone())
                .await
                .unwrap()
                .unwrap();
        } // both connections closed

        let reopened = Storage::open_db(dir.path()).unwrap();

        let sessions = reopened
            .sessions
            .query(&SessionQuery::default())
            .await
            .unwrap();
        assert_eq!(sessions.len(), 1);
        let last = reopened.estimates.last_of(task.id).await.unwrap().unwrap();
        assert_eq!(last.estimate, estimate);
    }
}
