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
pub mod relocate;
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
    /// Open (or create) `DB_FILE_NAME` in `data_dir` (created `0700` if
    /// missing).
    pub fn open_db(data_dir: &Path) -> Res<Self> {
        create_private_dir(data_dir)?;
        let path = data_dir.join(sqlite::DB_FILE_NAME);
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

/// `dir` and its missing parents; the ones created here get `0700`
/// (unix), existing ones are left as they are.
pub(crate) fn create_private_dir(dir: &Path) -> Res<()> {
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
    Ok(builder.create(dir)?)
}

#[cfg(test)]
mod tests {
    use crate::{
        estimate::store::{EstimateStore, NewEstimate, Reason},
        model::{
            id::NodeId,
            sessions::{SessionQuery, SessionStore, TaskRef},
            time::Minutes,
        },
        test_util::at,
    };

    use super::*;

    #[test]
    fn open_db_creates_the_file_in_the_data_dir() {
        let dir = tempfile::tempdir().unwrap();

        Storage::open_db(dir.path()).unwrap();

        assert!(dir.path().join(sqlite::DB_FILE_NAME).exists());
    }

    #[cfg(unix)]
    #[test]
    fn open_db_creates_a_missing_data_dir_private() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let data = tmp.path().join("share").join("udo");

        Storage::open_db(&data).unwrap();

        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&data), 0o700);
        assert!(data.join(sqlite::DB_FILE_NAME).exists());
    }

    /// The user's own dir (`UDO_DATA=~/sync/udo`) keeps the mode it has.
    #[cfg(unix)]
    #[test]
    fn open_db_leaves_an_existing_dir_mode_alone() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        std::fs::set_permissions(tmp.path(), std::fs::Permissions::from_mode(0o755)).unwrap();

        Storage::open_db(tmp.path()).unwrap();

        let mode = std::fs::metadata(tmp.path()).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o755);
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
            prior_source: None,
            frozen_at: None,
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
