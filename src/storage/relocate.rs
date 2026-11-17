//! Moving a `udo.db` out of the config root into the data dir, once (#93):
//! copied, checked, placed, then the old file is renamed to a `.bak` that
//! udo never deletes. Runs before anything else opens a database.

use std::{
    error::Error,
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

use rusqlite::{Connection, ErrorCode, OpenFlags};

use crate::{
    Res,
    dir::{Dirs, is_same_root},
    model::time::{self, Time},
    storage::{
        create_private_dir,
        sqlite::{self, DB_FILE_NAME},
    },
};

/// Held while moving: two udo processes never move at once.
const LOCK_FILE_NAME: &str = ".udo-move.lock";

/// A finished move: the database is now at `to`, the old file at `kept`.
#[derive(Debug, PartialEq)]
pub struct Moved {
    pub to: PathBuf,
    pub kept: PathBuf,
}

/// The old database is open in another connection (a running TUI, also
/// an older one): nothing was moved.
#[derive(Debug)]
pub struct InUse {
    pub old: PathBuf,
    /// A plain connection to `old`, opened under the move lock: while it
    /// lives no udo moves the file. None when a second database sits in
    /// the data dir (using `old` in place would split the sessions).
    hold: Option<Connection>,
}

impl std::fmt::Display for InUse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "udo.db at {} is in use by another udo (a running TUI?): \
             close it and run again; nothing was moved",
            self.old.display()
        )
    }
}

impl Error for InUse {}

/// Where this run opens `udo.db`, and what to tell the user about it.
/// Keep it alive as long as the database is open: in place, it holds the
/// old file so no other udo moves it meanwhile.
#[derive(Debug)]
pub struct Settled {
    pub db_dir: PathBuf,
    pub notice: Option<String>,
    _hold: Option<Connection>,
}

/// At startup: move an old `udo.db` (`move_old_db_at`). While another udo
/// has it open, this run uses it where it is (a later start moves it);
/// every other refusal stays an error. No busy wait: a holder (a running
/// TUI) never lets go meanwhile, and every udo call would wait first.
pub fn settle_old_db(dirs: &Dirs) -> Res<Settled> {
    settle_old_db_at(dirs, time::now(), Duration::ZERO)
}

/// `settle_old_db` at `at`, waiting up to `busy` (see `move_old_db_at`).
fn settle_old_db_at(dirs: &Dirs, at: Time, busy: Duration) -> Res<Settled> {
    match move_old_db_at(dirs, at, busy) {
        Ok(moved) => Ok(Settled {
            db_dir: dirs.data.clone(),
            notice: moved.map(|moved| {
                format!(
                    "moved udo.db to {}; old copy kept at {} (delete it once you are happy)",
                    dirs.data.display(),
                    moved.kept.display()
                )
            }),
            _hold: None,
        }),
        Err(e) => {
            let in_use = *e.downcast::<InUse>()?;
            let Some(hold) = in_use.hold else {
                return Err(Box::new(in_use));
            };
            Ok(Settled {
                db_dir: dirs.root.clone(),
                notice: Some(format!(
                    "udo.db at {} is in use by another udo (a running TUI?): \
                     using it there for now; it moves to {} once no udo has it open",
                    in_use.old.display(),
                    dirs.data.display()
                )),
                _hold: Some(hold),
            })
        }
    }
}

/// Move `<root>/udo.db` to `<data>/udo.db` if `dirs` asks for it
/// (`Dirs::move_old_db`) and there is one. None: nothing to move (also when
/// another udo just moved it). Err: nothing was moved, the old file is as
/// it was (in use, broken, or a different database already in the data
/// dir; in use is an `InUse`). `at` names the `.bak`; waits up to `busy`
/// for the old database to be free.
fn move_old_db_at(dirs: &Dirs, at: Time, busy: Duration) -> Res<Option<Moved>> {
    let old = dirs.root.join(DB_FILE_NAME);
    if !dirs.move_old_db || !old.exists() || is_same_root(&dirs.root, &dirs.data) {
        return Ok(None); // the common case: no lock, no dir created
    }
    create_private_dir(&dirs.data)?;
    let lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(dirs.data.join(LOCK_FILE_NAME))?;
    lock.lock()?; // released when `lock` is dropped
    if !old.exists() {
        return Ok(None); // another udo moved it while we waited
    }

    let new = dirs.data.join(DB_FILE_NAME);
    let Some(conn) = open_exclusive(&old, busy)? else {
        // still under the lock: the hold keeps every other udo from moving
        let hold = if new.exists() {
            None
        } else {
            Some(hold_open(&old)?)
        };
        return Err(Box::new(InUse { old, hold }));
    };
    // everything into the main file: it becomes the `.bak`, its WAL goes
    checkpoint(&conn)?;
    if new.exists() {
        // a move that crashed before the rename left the same data there
        if !same_content(&conn, &open_existing(&new)?)? {
            return Err(both(&old, &new, &dirs.root));
        }
    } else {
        copy(&conn, &old, &new)?;
    }
    let kept = finish(conn, &old, at)?;
    Ok(Some(Moved { to: new, kept }))
}

/// `path`, opened so no other connection may use it while this one lives
/// (`locking_mode=EXCLUSIVE`, taken by a first `BEGIN EXCLUSIVE`). Another
/// connection that merely has it open, idle, makes this fail: verified
/// 2026-11-16, see `refuses_while_another_connection_is_open`. None: in use.
fn open_exclusive(path: &Path, busy: Duration) -> Res<Option<Connection>> {
    let conn = open_existing(path)?;
    conn.busy_timeout(busy)?;
    conn.pragma_update(None, "locking_mode", "EXCLUSIVE")?;
    match conn.execute_batch("BEGIN EXCLUSIVE; COMMIT;") {
        Err(rusqlite::Error::SqliteFailure(e, _))
            if matches!(e.code, ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked) =>
        {
            Ok(None)
        }
        result => Ok(result.map(|()| Some(conn))?),
    }
}

/// `path` opened and read once: like any idle reader (a TUI), it makes
/// `open_exclusive` fail for as long as it lives.
fn hold_open(path: &Path) -> Res<Connection> {
    let conn = open_existing(path)?;
    conn.query_row("SELECT count(*) FROM sqlite_master", [], |r| {
        r.get::<_, i64>(0)
    })?;
    Ok(conn)
}

/// An existing database (never creates one), read-write.
fn open_existing(path: &Path) -> Res<Connection> {
    Ok(Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?)
}

/// Write the whole WAL into the main file and empty it.
fn checkpoint(conn: &Connection) -> Res<()> {
    let busy: i64 = conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |r| r.get(0))?;
    if busy != 0 {
        return Err("could not checkpoint udo.db; nothing was moved".into());
    }
    Ok(())
}

/// Copy the database of `conn` (at `old`) to `new`: checked first, written
/// to a temp file of its own, the copy checked, synced, then placed
/// without replacing anything. The temp file goes in every case.
fn copy(conn: &Connection, old: &Path, new: &Path) -> Res<()> {
    integrity(conn, old)?;
    let tmp = new.with_file_name(format!("{DB_FILE_NAME}.{}.tmp", std::process::id()));
    let Some(tmp_text) = tmp.to_str() else {
        return Err(format!("{} is not UTF-8; nothing was moved", tmp.display()).into());
    };
    let _ = fs::remove_file(&tmp); // left by a crashed try with the same pid
    let placed = (|| -> Res<()> {
        conn.execute("VACUUM INTO ?1", [tmp_text])?;
        {
            let copy = open_existing(&tmp)?;
            integrity(&copy, &tmp)?;
            if !same_content(conn, &copy)? {
                return Err(broken(old, "copy differs"));
            }
        }
        fs::File::open(&tmp)?.sync_all()?;
        sqlite::place(&tmp, new, |from, to| fs::hard_link(from, to))?;
        sqlite::sync_dir(new);
        // `place` keeps a file that was there first: only ours may count
        if !same_content(conn, &open_existing(new)?)? {
            return Err(broken(new, "not the copy"));
        }
        Ok(())
    })();
    let _ = fs::remove_file(&tmp);
    placed
}

/// Rename the old file to `udo.db.moved.<at>.bak` next to it, then close.
/// Unix: while still holding the exclusive lock (no one can open it in
/// between); elsewhere an open file cannot be renamed, so close first.
/// Then the empty `-wal` / `-shm` left under the old name go, but only if
/// no new `udo.db` appeared there (an older udo's, whose WAL is live).
fn finish(conn: Connection, old: &Path, at: Time) -> Res<PathBuf> {
    let mut name = old.file_name().unwrap_or_default().to_os_string();
    name.push(format!(".moved.{}.bak", at.format("%Y%m%d-%H%M%S")));
    let kept = old.with_file_name(name);
    #[cfg(not(unix))]
    drop(conn);
    fs::rename(old, &kept)?;
    #[cfg(unix)]
    drop(conn);
    sqlite::sync_dir(&kept);
    if !old.exists() {
        for suffix in ["-wal", "-shm"] {
            let mut leftover = old.as_os_str().to_os_string();
            leftover.push(suffix);
            let _ = fs::remove_file(leftover);
        }
    }
    Ok(kept)
}

/// `PRAGMA integrity_check` answers exactly `ok`.
fn integrity(conn: &Connection, path: &Path) -> Res<()> {
    let answer: String = conn.query_row("PRAGMA integrity_check", [], |r| r.get(0))?;
    if answer != "ok" {
        return Err(broken(path, &answer));
    }
    Ok(())
}

/// Same schema version, same tables, same number of rows in each: the same
/// data as far as a move is concerned (the copy itself passed
/// `integrity_check`). A database an older udo made after a move (empty)
/// or wrote to differs.
fn same_content(a: &Connection, b: &Connection) -> Res<bool> {
    Ok(summary(a)? == summary(b)?)
}

/// Schema version and `(table, rows)` sorted by name.
fn summary(conn: &Connection) -> Res<(usize, Vec<(String, i64)>)> {
    let names: Vec<String> = conn
        .prepare("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name")?
        .query_map([], |r| r.get(0))?
        .collect::<Result<_, _>>()?;
    let mut tables = Vec::with_capacity(names.len());
    for name in names {
        let sql = format!("SELECT count(*) FROM \"{}\"", name.replace('"', "\"\""));
        tables.push((name, conn.query_row(&sql, [], |r| r.get(0))?));
    }
    Ok((sqlite::user_version(conn)?, tables))
}

fn broken(path: &Path, detail: &str) -> Box<dyn Error> {
    format!(
        "{} failed its integrity check ({detail}); nothing was moved",
        path.display()
    )
    .into()
}

fn both(old: &Path, new: &Path, root: &Path) -> Box<dyn Error> {
    format!(
        "two databases: {} and {}: move {} away (after checking it) to use the second, \
         or set UDO_DATA={} to keep using the first",
        old.display(),
        new.display(),
        old.display(),
        root.display()
    )
    .into()
}

#[cfg(test)]
mod tests {
    use std::{sync::Arc, thread};

    use tempfile::TempDir;

    use super::*;
    use crate::{
        model::{
            id::NodeId,
            sessions::{SessionStore, TaskRef},
        },
        storage::Storage,
        test_util::{at, parse_time},
    };

    const NOW: &str = "2026-11-14T12:00:00+01:00";
    const KEPT: &str = "udo.db.moved.20261114-120000.bak";

    /// A tmp dir with `cfg/udo` (root) and `share/udo` (data, not created).
    fn setup() -> (TempDir, Dirs) {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("cfg").join("udo");
        fs::create_dir_all(&root).unwrap();
        let data = tmp.path().join("share").join("udo");
        (
            tmp,
            Dirs {
                root,
                data,
                move_old_db: true,
            },
        )
    }

    fn move_now(dirs: &Dirs) -> Res<Option<Moved>> {
        move_old_db_at(dirs, parse_time(NOW), Duration::ZERO)
    }

    fn settle_now(dirs: &Dirs) -> Res<Settled> {
        settle_old_db_at(dirs, parse_time(NOW), Duration::ZERO)
    }

    /// A real `udo.db` (all migrations) in `dir` with `n` sessions.
    async fn seed(dir: &Path, n: u32) {
        seed_from(dir, 8, n).await;
    }

    /// `seed`, the sessions from `first` o'clock on, one per hour.
    async fn seed_from(dir: &Path, first: u32, n: u32) {
        let storage = Storage::open_db(dir).unwrap();
        let task = TaskRef {
            id: NodeId::new(),
            name: "lab 3".into(),
            description: String::new(),
            container_id: NodeId::new(),
            container_dir: "uni/cs".into(),
        };
        for i in 0..n {
            storage
                .sessions
                .add(task.clone(), at(first + i, 0), at(first + i, 30))
                .await
                .unwrap();
        }
    }

    fn sessions(db: &Path) -> i64 {
        let conn = Connection::open_with_flags(db, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
        conn.query_row("SELECT count(*) FROM sessions", [], |r| r.get(0))
            .unwrap()
    }

    fn names(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn nothing_to_move_without_old_db() {
        let (_tmp, dirs) = setup();

        assert_eq!(move_now(&dirs).unwrap(), None);
        assert!(!dirs.data.exists()); // not even the dir
    }

    #[tokio::test]
    async fn skips_unless_asked() {
        let (_tmp, mut dirs) = setup();
        seed(&dirs.root, 1).await;
        dirs.move_old_db = false;

        assert_eq!(move_now(&dirs).unwrap(), None);
        assert!(dirs.root.join(DB_FILE_NAME).exists());
        assert!(!dirs.data.exists());
    }

    #[tokio::test]
    async fn skips_when_root_is_data() {
        let (_tmp, mut dirs) = setup();
        seed(&dirs.root, 1).await;
        dirs.data = dirs.root.clone();

        assert_eq!(move_now(&dirs).unwrap(), None);
        assert_eq!(sessions(&dirs.root.join(DB_FILE_NAME)), 1);
    }

    #[tokio::test]
    async fn moves_and_keeps_the_old_file() {
        let (_tmp, dirs) = setup();
        seed(&dirs.root, 3).await;
        fs::write(dirs.root.join(crate::UDO_FILE_NAME), "kind = \"Root\"").unwrap();

        let moved = move_now(&dirs).unwrap().unwrap();

        assert_eq!(
            moved,
            Moved {
                to: dirs.data.join(DB_FILE_NAME),
                kept: dirs.root.join(KEPT)
            }
        );
        assert_eq!(names(&dirs.root), [".udo.toml", KEPT]); // no -wal / -shm left
        assert_eq!(names(&dirs.data), [".udo-move.lock", "udo.db"]); // no tmp left
        assert_eq!(sessions(&moved.to), 3);
        assert_eq!(sessions(&moved.kept), 3); // the old file, whole (opening it adds its -wal / -shm)
        let new = Connection::open(&moved.to).unwrap();
        assert_eq!(sqlite::user_version(&new).unwrap(), 3);
    }

    /// The moved file opens with the normal open (WAL, migrations) and
    /// keeps working.
    #[tokio::test]
    async fn the_moved_database_opens_as_usual() {
        let (_tmp, dirs) = setup();
        seed(&dirs.root, 2).await;
        move_now(&dirs).unwrap();

        seed_from(&dirs.data, 14, 1).await;

        assert_eq!(sessions(&dirs.data.join(DB_FILE_NAME)), 3);
    }

    #[tokio::test]
    async fn moved_once_then_nothing_to_move() {
        let (_tmp, dirs) = setup();
        seed(&dirs.root, 1).await;
        move_now(&dirs).unwrap().unwrap();

        assert_eq!(move_now(&dirs).unwrap(), None);
    }

    /// A running TUI (any udo version) keeps the file open, idle.
    #[tokio::test]
    async fn refuses_while_another_connection_is_open() {
        let (_tmp, dirs) = setup();
        seed(&dirs.root, 2).await;
        let old = dirs.root.join(DB_FILE_NAME);
        let tui = sqlite::open(&old).unwrap(); // WAL, no transaction
        let before = names(&dirs.root);

        let err = move_now(&dirs).unwrap_err().to_string();

        assert!(err.contains("is in use by another udo"), "{err}");
        assert!(err.contains("nothing was moved"), "{err}");
        assert_eq!(names(&dirs.root), before);
        assert_eq!(names(&dirs.data), [".udo-move.lock"]); // no db, no tmp
        drop(tui);
        assert_eq!(sessions(&old), 2);
        assert!(move_now(&dirs).unwrap().is_some()); // once it is closed
    }

    /// An older TUI still has it open: this run uses it where it is, a
    /// later one moves it.
    #[tokio::test]
    async fn falls_back_to_the_old_db_while_in_use() {
        let (_tmp, dirs) = setup();
        seed(&dirs.root, 2).await;
        let tui = sqlite::open(&dirs.root.join(DB_FILE_NAME)).unwrap();
        let before = names(&dirs.root);

        let settled = settle_now(&dirs).unwrap();

        assert_eq!(settled.db_dir, dirs.root);
        let notice = settled.notice.clone().unwrap();
        assert!(notice.contains("is in use by another udo"), "{notice}");
        assert!(notice.contains("using it there for now"), "{notice}");
        assert!(
            notice.contains(&dirs.data.display().to_string()),
            "{notice}"
        );
        assert_eq!(names(&dirs.root), before);
        drop(settled);
        drop(tui);
        let settled = settle_now(&dirs).unwrap();
        assert_eq!(settled.db_dir, dirs.data);
        let notice = settled.notice.unwrap();
        assert!(notice.starts_with("moved udo.db to "), "{notice}");
    }

    /// The run in place keeps the old file open: a holder that quits
    /// meanwhile must not let another udo move it away under this run
    /// (which would then create a fresh, empty one in the root).
    #[tokio::test]
    async fn fallback_keeps_the_old_db_from_moving_until_dropped() {
        let (_tmp, dirs) = setup();
        seed(&dirs.root, 2).await;
        let tui = sqlite::open(&dirs.root.join(DB_FILE_NAME)).unwrap();

        let settled = settle_now(&dirs).unwrap();
        drop(tui);

        let err = move_now(&dirs).unwrap_err().to_string();
        assert!(err.contains("is in use by another udo"), "{err}");
        drop(settled);
        assert!(move_now(&dirs).unwrap().is_some());
    }

    /// An older udo made a fresh one in the root after the move and holds
    /// it: using it in place would split the sessions, so stop.
    #[tokio::test]
    async fn settle_refuses_in_use_next_to_a_second_database() {
        let (_tmp, dirs) = setup();
        seed(&dirs.root, 1).await;
        seed(&dirs.data, 4).await;
        let old_tui = sqlite::open(&dirs.root.join(DB_FILE_NAME)).unwrap();

        let err = settle_now(&dirs).unwrap_err().to_string();

        assert!(err.contains("is in use by another udo"), "{err}");
        assert!(err.contains("close it and run again"), "{err}");
        drop(old_tui);
        assert_eq!(sessions(&dirs.root.join(DB_FILE_NAME)), 1);
        assert_eq!(sessions(&dirs.data.join(DB_FILE_NAME)), 4);
    }

    /// A holder never lets go while it runs: every udo meanwhile (hooks
    /// too) must not wait the busy timeout first.
    #[tokio::test]
    async fn settle_does_not_wait_for_a_holder() {
        let (_tmp, dirs) = setup();
        seed(&dirs.root, 2).await;
        let _tui = sqlite::open(&dirs.root.join(DB_FILE_NAME)).unwrap();

        let start = std::time::Instant::now();
        let settled = settle_old_db(&dirs).unwrap();

        assert_eq!(settled.db_dir, dirs.root);
        assert!(
            start.elapsed() < Duration::from_secs(1),
            "{:?}",
            start.elapsed()
        );
    }

    #[test]
    fn settles_on_the_data_dir_with_nothing_to_move() {
        let (_tmp, dirs) = setup();

        let settled = settle_now(&dirs).unwrap();

        assert_eq!(settled.db_dir, dirs.data);
        assert_eq!(settled.notice, None);
    }

    #[tokio::test]
    async fn settle_still_stops_on_two_databases() {
        let (_tmp, dirs) = setup();
        seed(&dirs.root, 1).await;
        seed(&dirs.data, 4).await;

        let err = settle_now(&dirs).unwrap_err().to_string();

        assert!(err.starts_with("two databases: "), "{err}");
    }

    /// An older udo after the move made a fresh one in the root.
    #[tokio::test]
    async fn refuses_two_different_databases() {
        let (_tmp, dirs) = setup();
        seed(&dirs.root, 1).await;
        seed(&dirs.data, 4).await;

        let err = move_now(&dirs).unwrap_err().to_string();

        assert!(err.starts_with("two databases: "), "{err}");
        assert!(
            err.contains(&format!("UDO_DATA={}", dirs.root.display())),
            "{err}"
        );
        assert_eq!(sessions(&dirs.root.join(DB_FILE_NAME)), 1);
        assert_eq!(sessions(&dirs.data.join(DB_FILE_NAME)), 4);
        assert!(!dirs.root.join(KEPT).exists());
    }

    /// A crash after placing the copy, before the rename.
    #[tokio::test]
    async fn finishes_a_half_done_move() {
        let (_tmp, dirs) = setup();
        seed(&dirs.root, 2).await;
        fs::create_dir_all(&dirs.data).unwrap();
        let new = dirs.data.join(DB_FILE_NAME);
        Connection::open(dirs.root.join(DB_FILE_NAME))
            .unwrap()
            .execute("VACUUM INTO ?1", [new.to_str().unwrap()])
            .unwrap();

        let moved = move_now(&dirs).unwrap().unwrap();

        assert_eq!(moved.kept, dirs.root.join(KEPT));
        assert_eq!(sessions(&new), 2);
        assert!(!dirs.root.join(DB_FILE_NAME).exists());
    }

    #[test]
    fn refuses_a_broken_database() {
        let (_tmp, dirs) = setup();
        let old = dirs.root.join(DB_FILE_NAME);
        let mut garbage = b"SQLite format 3\0".to_vec();
        garbage.resize(4096, 0xAB);
        fs::write(&old, &garbage).unwrap();

        assert!(move_now(&dirs).is_err());
        assert_eq!(fs::read(&old).unwrap(), garbage);
        assert!(!dirs.data.join(DB_FILE_NAME).exists());
        assert!(!dirs.root.join(KEPT).exists());
    }

    /// Two udo processes starting at once: one moves, the other waits for
    /// the lock and finds nothing left to move.
    #[tokio::test]
    async fn two_at_once_move_once() {
        let (_tmp, dirs) = setup();
        seed(&dirs.root, 5).await;
        let dirs = Arc::new(dirs);

        let results: Vec<_> = (0..2)
            .map(|_| {
                let dirs = Arc::clone(&dirs);
                thread::spawn(move || {
                    move_old_db_at(&dirs, parse_time(NOW), Duration::from_secs(5))
                        .map_err(|e| e.to_string())
                })
            })
            .collect::<Vec<_>>()
            .into_iter()
            .map(|t| t.join().unwrap().unwrap())
            .collect();

        assert_eq!(
            results.iter().filter(|r| r.is_some()).count(),
            1,
            "{results:?}"
        );
        assert_eq!(sessions(&dirs.data.join(DB_FILE_NAME)), 5);
        assert_eq!(names(&dirs.root), [KEPT]);
    }
}
