use std::path::Path;

use crate::{
    Res,
    model::time::now,
    storage::sessions::{Sessions, memory::MemorySessions, sqlite::SqliteSessions},
};

mod edit_kind;
pub mod sessions;
mod sql;
pub mod sqlite;
mod time;

/// Every store udo uses, opened once at startup (sessions now, the tree
/// later).
pub struct Storage {
    pub sessions: Sessions,
}

impl Storage {
    /// Open (or create) `DB_FILE_NAME` in the root dir.
    pub fn open_db(root_dir: &Path) -> Res<Self> {
        let conn = sqlite::open(&root_dir.join(sqlite::DB_FILE_NAME))?;
        Ok(Self {
            sessions: Sessions::Sqlite(SqliteSessions::new(conn, now)),
        })
    }

    /// Nothing on disk (tests).
    pub fn in_memory() -> Self {
        Self {
            sessions: Sessions::Memory(MemorySessions::new(now)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_db_creates_the_file_in_the_root() {
        let dir = tempfile::tempdir().unwrap();

        Storage::open_db(dir.path()).unwrap();

        assert!(dir.path().join(sqlite::DB_FILE_NAME).exists());
    }
}
