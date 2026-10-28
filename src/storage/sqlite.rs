//! The one database (`udo.db`): connection setup and migrations, shared by
//! every area (sessions now, the tree later).

use std::{
    fs, io,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    thread,
    time::{Duration, Instant},
};

use rusqlite::{Connection, ErrorCode, OpenFlags, Transaction, TransactionBehavior};

use crate::{
    Res,
    model::time::{self, Time},
};

/// The database file, in the root dir.
pub const DB_FILE_NAME: &str = "udo.db";

/// Schema changes, in order. Never edit one that has shipped; append.
/// `PRAGMA user_version` counts how many have run.
const MIGRATIONS: &[&str] = &[
    include_str!("sqlite_migrations/001_sessions.sql"),
    include_str!("sqlite_migrations/002_estimates.sql"),
    include_str!("sqlite_migrations/003_estimate_prior_source.sql"),
];
const BUSY_TIMEOUT_SECS: u64 = 5;

/// Open (or create) the database file, set it up, migrate. Before a
/// migration an existing database is copied next to it (`backup_path`,
/// #78): migrations are one-way, an older udo refuses the result.
pub fn open(path: &Path) -> Res<Connection> {
    open_with(path, |db, version| backup_path(db, version, time::now()))
}

/// `open`, the copy going to `backup(path, old version)` (called only when
/// a migration is due).
fn open_with(path: &Path, backup: impl Fn(&Path, usize) -> PathBuf) -> Res<Connection> {
    let mut conn = Connection::open(path)?;
    // first: switching to WAL needs a lock too, so it must already wait for one
    conn.busy_timeout(Duration::from_secs(BUSY_TIMEOUT_SECS))?; // a second writer retries instead of failing
    enable_wal(&conn)?; // readers never wait on a writer
    let version = user_version(&conn)?;
    if 0 < version && version < MIGRATIONS.len() {
        let to = backup(path, version);
        // `VACUUM INTO` takes the path as text: a path that is not UTF-8
        // gets no copy and is migrated as before #78, rather than not
        // opened at all. Before `setup`: `VACUUM` cannot run inside its
        // transaction.
        if let Some(text) = to.to_str() {
            back_up(&conn, text, version).map_err(|e| {
                format!(
                    "could not back up {} to {} before upgrading it (schema {version} -> {}): {e}; it was not upgraded",
                    path.display(),
                    to.display(),
                    MIGRATIONS.len()
                )
            })?;
        }
    }
    setup(&mut conn)?;
    Ok(conn)
}

/// Where the copy of `db` at `version`, made at `at`, goes: next to it,
/// `udo.db.v2.20261028-014000.bak` (local time). Each upgrade gets its own
/// copy (also after a roll back and a later upgrade); udo never deletes
/// one.
fn backup_path(db: &Path, version: usize, at: Time) -> PathBuf {
    let mut name = db.file_name().unwrap_or_default().to_os_string();
    name.push(format!(".v{version}.{}.bak", at.format("%Y%m%d-%H%M%S")));
    db.with_file_name(name)
}

/// A consistent copy of the database, read at schema `version`, to `to`
/// (`VACUUM INTO`: SQLite writes it, the WAL included, while others may
/// write), synced to disk before the migration may commit. Written to a
/// file of its own first, then put at `to` (`place`): of two processes
/// copying in the same second, one copy stays. A copy that is no longer at
/// `version` is dropped: another process migrated in between (and, as
/// every one does, copied first), so a `v2` copy always holds schema 2. A
/// crash in the middle can leave the temp file behind; it is never read.
fn back_up(conn: &Connection, to: &str, version: usize) -> Res<()> {
    // unique per process and per call (threads share the process id)
    static CALLS: AtomicU64 = AtomicU64::new(0);
    let call = CALLS.fetch_add(1, Ordering::Relaxed);
    let tmp = format!("{to}.{}-{call}.tmp", std::process::id());
    let written = (|| -> Res<()> {
        conn.execute("VACUUM INTO ?1", [&tmp])?;
        if copied_version(Path::new(&tmp))? != version {
            return Ok(()); // migrated meanwhile: that process's copy is the one
        }
        fs::File::open(&tmp)?.sync_all()?;
        place(Path::new(&tmp), Path::new(to), |from, to| {
            fs::hard_link(from, to)
        })?;
        sync_dir(Path::new(to));
        Ok(())
    })();
    let _ = fs::remove_file(&tmp); // linked or dropped: the temp name goes
    written
}

/// The schema version of the copy at `copy`, read without touching it.
fn copied_version(copy: &Path) -> Res<usize> {
    let conn = Connection::open_with_flags(copy, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    user_version(&conn)
}

/// Put the finished copy `tmp` at `to`: a hard link (`link`) fails if `to`
/// exists, in one step; another process was first, theirs stays. Where
/// links are not supported (exFAT, some network drives), a rename if `to`
/// is not there yet: check and rename are two steps, so a process copying
/// in the same second may replace the other's copy, one of the same data.
fn place(tmp: &Path, to: &Path, link: impl Fn(&Path, &Path) -> io::Result<()>) -> io::Result<()> {
    match link(tmp, to) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => Ok(()),
        Err(_) if !to.exists() => fs::rename(tmp, to),
        Err(_) => Ok(()),
    }
}

/// Sync the directory of `file`, so its new name survives a power cut.
/// Best effort: not every system can open a directory for that.
fn sync_dir(file: &Path) {
    if let Some(dir) = file.parent() {
        let _ = fs::File::open(dir).and_then(|d| d.sync_all());
    }
}

/// The schema version (`PRAGMA user_version`): how many migrations ran.
fn user_version(conn: &Connection) -> Res<usize> {
    let v: u32 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
    Ok(v as usize)
}

/// Switch to WAL. On a fresh file this needs the file for itself for a
/// moment, and when several processes open it at once SQLite can answer
/// "busy" right away instead of waiting (it skips the busy handler there to
/// avoid a deadlock). So retry, for as long as the busy timeout.
fn enable_wal(conn: &Connection) -> Res<()> {
    let give_up = Instant::now() + Duration::from_secs(BUSY_TIMEOUT_SECS);
    loop {
        match conn.pragma_update(None, "journal_mode", "WAL") {
            Err(rusqlite::Error::SqliteFailure(e, _))
                if e.code == ErrorCode::DatabaseBusy && Instant::now() < give_up =>
            {
                thread::sleep(Duration::from_millis(10));
            }
            result => return Ok(result?),
        }
    }
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
    let done = user_version(&tx)?;
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

/// A transaction that takes the write lock right away (`BEGIN IMMEDIATE`):
/// checks and changes happen as one step, no other process in between.
pub(super) fn write_tx(conn: &mut Connection) -> rusqlite::Result<Transaction<'_>> {
    conn.transaction_with_behavior(TransactionBehavior::Immediate)
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

    /// `super::user_version`, the one reading of the schema version.
    fn user_version(conn: &Connection) -> usize {
        super::user_version(conn).unwrap()
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
             ('s1', 't1', 'lab 3', '', 'c1', '/uni', 0, 120, NULL, NULL, 'manual', 'manual', 0, 120, NULL, NULL)",
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
                 (?1, 't1', 'lab 3', '', 'c1', '/uni', 0, 120, NULL, NULL, 'manual', 'manual', 0, 120, NULL, NULL)",
                [id],
            )
        };

        running("s1").unwrap();

        assert!(running("s2").is_err()); // the `one_running` index
    }

    /// A `udo.db` from 0.1.x (only migration 001) opens with the newer udo:
    /// the estimates table is added, the recorded sessions stay.
    #[test]
    fn an_old_database_is_upgraded_and_keeps_its_sessions() {
        let (_dir, path) = db_path();
        {
            // what 0.1.x left behind: migration 001 only, user_version 1
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch(MIGRATIONS[0]).unwrap();
            conn.pragma_update(None, "user_version", 1).unwrap();
            conn.execute(
                "INSERT INTO sessions VALUES
                 ('s1', 't1', 'lab 3', '', 'c1', '/uni', 0, 120, NULL, NULL, 'manual', 'manual', 0, 120, NULL, NULL)",
                (),
            )
            .unwrap();
        }

        let conn = open(&path).unwrap();

        assert_eq!(user_version(&conn), MIGRATIONS.len());
        let sessions: u32 = conn
            .query_row("SELECT count(*) FROM sessions", (), |r| r.get(0))
            .unwrap();
        assert_eq!(sessions, 1);
        let estimates: u32 = conn
            .query_row("SELECT count(*) FROM estimates", (), |r| r.get(0))
            .unwrap();
        assert_eq!(estimates, 0); // the table exists, empty
    }

    /// What 0.2.x / 0.3.x left behind at `path`: migrations 001 and 002,
    /// user_version 2, one estimate row.
    fn v2_database(path: &Path) {
        let conn = Connection::open(path).unwrap();
        conn.execute_batch(MIGRATIONS[0]).unwrap();
        conn.execute_batch(MIGRATIONS[1]).unwrap();
        conn.pragma_update(None, "user_version", 2).unwrap();
        conn.execute(
            "INSERT INTO estimates VALUES
             ('e1', 't1', 90, 'prior', 1, 0, 0, 90, 'created', 0, 120)",
            (),
        )
        .unwrap();
    }

    /// The `*.bak` files next to `path`; asserts no temp file of a copy is
    /// left over.
    fn copies(path: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(path.parent().unwrap())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert!(!names.iter().any(|n| n.ends_with(".tmp")), "{names:?}");
        names.retain(|n| n.ends_with(".bak"));
        names.sort();
        names
    }

    /// A `udo.db` from 0.2.x (migrations 001 and 002) opens with the newer udo:
    /// the estimate rows stay, without a prior source (#60).
    #[test]
    fn an_estimates_database_from_0_2_is_upgraded() {
        let (_dir, path) = db_path();
        v2_database(&path);

        let conn = open(&path).unwrap();

        assert_eq!(user_version(&conn), MIGRATIONS.len());
        let source: Option<String> = conn
            .query_row("SELECT prior_source FROM estimates", (), |r| r.get(0))
            .unwrap();
        assert_eq!(source, None);
    }

    // ---------- the copy before a migration (#78) ----------

    /// Copies go to `udo.db.v2.<at>.bak`, `at` fixed: names a test knows.
    fn at(at: &'static str) -> impl Fn(&Path, usize) -> PathBuf {
        move |db, version| backup_path(db, version, crate::test_util::parse_time(at))
    }

    /// Before migrating, the old database is copied next to it, named after
    /// its version and the time: a usable database at that version, with
    /// its rows.
    #[test]
    fn a_migration_keeps_a_copy_of_the_old_database() {
        let (_dir, path) = db_path();
        v2_database(&path);

        let conn = open(&path).unwrap();

        assert_eq!(user_version(&conn), MIGRATIONS.len());
        let [name] = &copies(&path)[..] else {
            panic!("one copy: {:?}", copies(&path));
        };
        assert!(
            name.starts_with("udo.db.v2.") && name.ends_with(".bak"),
            "{name}"
        );
        let copy = Connection::open(path.with_file_name(name)).unwrap();
        assert_eq!(user_version(&copy), 2);
        let rows: u32 = copy
            .query_row("SELECT count(*) FROM estimates", (), |r| r.get(0))
            .unwrap();
        assert_eq!(rows, 1);
        let migrated = copy.query_row("SELECT prior_source FROM estimates", (), |r| {
            r.get::<_, Option<String>>(0)
        });
        assert!(migrated.is_err(), "the copy is from before migration 003");
    }

    #[test]
    fn a_fresh_database_gets_no_copy() {
        let (_dir, path) = db_path();

        open(&path).unwrap();

        assert!(copies(&path).is_empty());
    }

    #[test]
    fn an_up_to_date_database_gets_no_copy() {
        let (_dir, path) = db_path();
        open(&path).unwrap();

        open(&path).unwrap();

        assert!(copies(&path).is_empty());
    }

    /// A copy from an earlier upgrade (then rolled back, the old udo used
    /// for a while) stays as it is; this upgrade adds its own.
    #[test]
    fn an_earlier_copy_stays_and_a_new_one_is_added() {
        let (_dir, path) = db_path();
        v2_database(&path);
        let earlier = path.with_file_name("udo.db.v2.20261001-090000.bak");
        std::fs::write(&earlier, "the earlier copy").unwrap();

        let conn = open_with(&path, at("2026-10-28T01:40:00+01:00")).unwrap();

        assert_eq!(user_version(&conn), MIGRATIONS.len());
        assert_eq!(
            copies(&path),
            [
                "udo.db.v2.20261001-090000.bak",
                "udo.db.v2.20261028-014000.bak"
            ]
        );
        assert_eq!(
            std::fs::read_to_string(&earlier).unwrap(),
            "the earlier copy"
        );
    }

    /// Several processes (TUI, hooks) opening an old database at once, in
    /// the same second: all succeed, one copy, at the old version. (Across
    /// a second boundary each second gets its copy: fine, never wrong.)
    #[test]
    fn upgrading_concurrently_copies_once() {
        const OPENERS: usize = 8;
        let (_dir, path) = db_path();
        v2_database(&path);
        let start = Arc::new(Barrier::new(OPENERS));

        let openers: Vec<_> = (0..OPENERS)
            .map(|_| {
                let (path, start) = (path.clone(), Arc::clone(&start));
                thread::spawn(move || {
                    start.wait(); // all at once
                    open_with(&path, at("2026-10-28T01:40:00+01:00"))
                        .map(|_| ())
                        .map_err(|e| e.to_string())
                })
            })
            .collect();

        for opener in openers {
            assert_eq!(opener.join().unwrap(), Ok(()));
        }
        assert_eq!(copies(&path), ["udo.db.v2.20261028-014000.bak"]);
        let copy = Connection::open(path.with_file_name("udo.db.v2.20261028-014000.bak")).unwrap();
        assert_eq!(user_version(&copy), 2);
    }

    /// Read at schema 2, but another process migrated before the copy was
    /// taken: the copy holds schema 3, so it is dropped (that process
    /// copied first); a `v2` copy never holds a migrated database.
    #[test]
    fn a_copy_of_a_database_migrated_meanwhile_is_dropped() {
        let (dir, path) = db_path();
        v2_database(&path);
        open_with(&path, at("2026-10-28T01:40:00+01:00")).unwrap(); // the other process
        let conn = Connection::open(&path).unwrap(); // now at the latest schema
        let to = dir.path().join("udo.db.v2.20261028-014001.bak");

        back_up(&conn, to.to_str().unwrap(), 2).unwrap(); // as read before

        assert!(!to.exists());
        assert_eq!(copies(&path), ["udo.db.v2.20261028-014000.bak"]); // theirs only
    }

    /// No copy, no migration: the database stays as it was, an older udo
    /// still opens it. The message names both files.
    #[test]
    fn a_failed_copy_refuses_the_migration() {
        let (dir, path) = db_path();
        v2_database(&path);
        let nowhere = dir.path().join("no such dir").join("udo.db.v2.bak");

        let err = open_with(&path, |_, _| nowhere.clone())
            .unwrap_err()
            .to_string();

        assert!(
            err.starts_with(&format!(
                "could not back up {} to {}",
                path.display(),
                nowhere.display()
            )),
            "{err}"
        );
        assert!(err.contains("schema 2 -> "), "{err}");
        assert!(err.ends_with("it was not upgraded"), "{err}");
        assert_eq!(user_version(&Connection::open(&path).unwrap()), 2);
    }

    #[test]
    fn the_copy_is_named_after_the_old_version_and_the_time() {
        let path = Path::new("/data/udo.db");
        let at = crate::test_util::parse_time("2026-10-28T01:40:00+01:00");

        assert_eq!(
            backup_path(path, 2, at),
            Path::new("/data/udo.db.v2.20261028-014000.bak")
        );
    }

    /// Where hard links are not supported (exFAT, some network drives) the
    /// copy is renamed into place; never over an existing file.
    #[test]
    fn without_hard_links_the_copy_is_renamed_never_over_one() {
        let dir = tempfile::tempdir().unwrap();
        let (tmp, to) = (dir.path().join("copy.tmp"), dir.path().join("copy.bak"));
        let unsupported = |_: &Path, _: &Path| Err(io::Error::from(io::ErrorKind::Unsupported));
        std::fs::write(&tmp, "new").unwrap();

        place(&tmp, &to, unsupported).unwrap();

        assert_eq!(std::fs::read_to_string(&to).unwrap(), "new");
        assert!(!tmp.exists());
        std::fs::write(&tmp, "newer").unwrap();
        place(&tmp, &to, unsupported).unwrap(); // `to` there: kept
        assert_eq!(std::fs::read_to_string(&to).unwrap(), "new");
    }

    #[test]
    fn another_process_first_keeps_its_copy() {
        let dir = tempfile::tempdir().unwrap();
        let (tmp, to) = (dir.path().join("copy.tmp"), dir.path().join("copy.bak"));
        std::fs::write(&tmp, "mine").unwrap();
        std::fs::write(&to, "theirs").unwrap();

        place(&tmp, &to, |a, b| fs::hard_link(a, b)).unwrap();

        assert_eq!(std::fs::read_to_string(&to).unwrap(), "theirs");
    }

    /// A data root whose path is not UTF-8 (possible on Linux): no copy
    /// (`VACUUM INTO` takes text), but udo still opens and migrates it.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_path_that_is_not_utf8_is_migrated_without_a_copy() {
        use std::{ffi::OsStr, os::unix::ffi::OsStrExt};
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join(OsStr::from_bytes(b"m\xfcller"));
        std::fs::create_dir(&root).unwrap();
        let path = root.join("udo.db");
        v2_database(&path);

        let conn = open(&path).unwrap();

        assert_eq!(user_version(&conn), MIGRATIONS.len());
        assert!(copies(&path).is_empty());
    }

    #[test]
    fn estimates_take_a_full_row() {
        let conn = open_in_memory().unwrap();

        conn.execute(
            "INSERT INTO estimates VALUES
             ('e1', 't1', 185, 'average', 1, 0, 1, 180, 'started', 0, 120, 'setting', 'c1', NULL, 0, 120)",
            (),
        )
        .unwrap();
        // NOT NULL holds: a row without counts is refused
        let missing = conn.execute(
            "INSERT INTO estimates (id, task_id, minutes, method, version, reason, at, at_offset)
             VALUES ('e2', 't1', 185, 'average', 1, 'started', 0, 120)",
            (),
        );
        assert!(missing.is_err());
    }
}
