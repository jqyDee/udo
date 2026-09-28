//! The one database (`udo.db`): connection setup and migrations, shared by
//! every area (sessions now, the tree later).

use std::{path::Path, time::Duration};

use rusqlite::{Connection, TransactionBehavior};

use crate::Res;

/// The database file, in the root dir.
pub const DB_FILE_NAME: &str = "udo.db";

/// Schema changes, in order. Never edit one that has shipped; append.
/// `PRAGMA user_version` counts how many have run.
const MIGRATIONS: &[&str] = &[include_str!("sqlite_migrations/001_sessions.sql")];
const BUSY_TIMEOUT_SECS: u64 = 5;

/// Open (or create) the database file, set it up, migrate.
pub fn open(path: &Path) -> Res<Connection> {
    let mut conn = Connection::open(path)?;
    // first: switching to WAL needs a lock too, so it must already wait for one
    conn.busy_timeout(Duration::from_secs(BUSY_TIMEOUT_SECS))?; // a second writer retries instead of failing
    conn.pragma_update(None, "journal_mode", "WAL")?; // readers never wait on a writer
    setup(&mut conn)?;
    Ok(conn)
}

/// A fresh, empty database in memory (tests).
pub fn open_in_memory() -> Res<Connection> {
    let mut conn = Connection::open_in_memory()?;
    setup(&mut conn)?;
    Ok(conn)
}

/// Per-connection settings, then the missing migrations. Refuses a database
/// written by a newer udo (more migrations than this one knows).
fn setup(conn: &mut Connection) -> Res<()> {
    conn.pragma_update(None, "foreign_keys", "ON")?; // per connection, not inside a transaction
    // write lock first: two processes opening a fresh file must not both migrate
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let done: u32 = tx.pragma_query_value(None, "user_version", |r| r.get(0))?;
    let done = done as usize;
    if done > MIGRATIONS.len() {
        return Err(format!(
            "udo.db is from a newer udo (schema {done}, known {})",
            MIGRATIONS.len()
        )
        .into());
    }
    // all pending ones in one transaction: never half-migrated
    for sql in &MIGRATIONS[done..] {
        tx.execute_batch(sql)?;
    }
    tx.pragma_update(None, "user_version", MIGRATIONS.len() as u32)?;
    tx.commit()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{
        path::PathBuf,
        sync::{Arc, Barrier},
        thread,
    };

    use tempfile::TempDir;

    use super::*;

    /// A path for `udo.db` in a fresh temp dir (kept alive by the `TempDir`).
    fn db_path() -> (TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("udo.db");
        (dir, path)
    }

    fn user_version(conn: &Connection) -> usize {
        let v: u32 = conn
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .unwrap();
        v as usize
    }

    #[test]
    fn open_creates_the_file_migrated_and_in_wal_mode() {
        let (_dir, path) = db_path();

        let conn = open(&path).unwrap();

        assert!(path.exists());
        assert_eq!(user_version(&conn), MIGRATIONS.len());
        let mode: String = conn
            .pragma_query_value(None, "journal_mode", |r| r.get(0))
            .unwrap();
        assert_eq!(mode, "wal");
    }

    #[test]
    fn reopening_keeps_the_data_and_migrates_once() {
        let (_dir, path) = db_path();
        let conn = open(&path).unwrap();
        conn.execute(
            "INSERT INTO sessions VALUES
             ('s1', 't1', 'lab 3', '', 'uni', 0, 120, NULL, NULL, 'manual', 0, 120, NULL, NULL)",
            (),
        )
        .unwrap();
        drop(conn);

        let conn = open(&path).unwrap(); // re-creating the tables would fail here

        assert_eq!(user_version(&conn), MIGRATIONS.len());
        let rows: u32 = conn
            .query_row("SELECT count(*) FROM sessions", (), |r| r.get(0))
            .unwrap();
        assert_eq!(rows, 1);
    }

    #[test]
    fn a_newer_schema_is_refused() {
        let (_dir, path) = db_path();
        open(&path)
            .unwrap()
            .pragma_update(None, "user_version", MIGRATIONS.len() as u32 + 1)
            .unwrap();

        let err = open(&path).unwrap_err().to_string();

        assert!(err.contains("newer udo"), "{err}");
    }

    /// Several processes (TUI, CLI, tmux hooks) opening a fresh file at the
    /// same moment: all succeed, the schema is created once.
    #[test]
    fn opening_a_fresh_file_concurrently_migrates_once() {
        const OPENERS: usize = 8;
        let (_dir, path) = db_path();
        let start = Arc::new(Barrier::new(OPENERS));

        let openers: Vec<_> = (0..OPENERS)
            .map(|_| {
                let (path, start) = (path.clone(), Arc::clone(&start));
                thread::spawn(move || {
                    start.wait(); // all at once
                    open(&path).map(|_| ()).map_err(|e| e.to_string())
                })
            })
            .collect();

        for opener in openers {
            assert_eq!(opener.join().unwrap(), Ok(()));
        }
        assert_eq!(user_version(&open(&path).unwrap()), MIGRATIONS.len());
    }

    #[test]
    fn foreign_keys_are_enforced() {
        let conn = open_in_memory().unwrap();

        let orphan = conn.execute(
            "INSERT INTO session_edits (id, session_id, at, at_offset, kind)
             VALUES ('e1', 'no such session', 0, 0, 'edit')",
            (),
        );

        assert!(orphan.is_err());
    }

    #[test]
    fn one_running_session_at_most() {
        let conn = open_in_memory().unwrap();
        let running = |id: &str| {
            conn.execute(
                "INSERT INTO sessions VALUES
                 (?1, 't1', 'lab 3', '', 'uni', 0, 120, NULL, NULL, 'manual', 0, 120, NULL, NULL)",
                [id],
            )
        };

        running("s1").unwrap();

        assert!(running("s2").is_err()); // the `one_running` index
    }
}
