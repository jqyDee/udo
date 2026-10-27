//! Estimate rows in `udo.db` (table `estimates`, migrations 002 and 003).

use std::sync::{Arc, Mutex};

use rusqlite::{Connection, OptionalExtension, Row, params, params_from_iter, types::Type};
use uuid::Uuid;

use crate::{
    estimate::store::{EstimateError, EstimateStore, NewEstimate, PriorSource, Recorded},
    model::{
        id::NodeId,
        time::{Clock, Minutes, Time},
    },
    storage::{
        sqlite::write_tx,
        time::{time_from_row, time_to_sql, to_ms},
    },
};

/// The estimate history in `udo.db`. Append-only: rows are inserted (a
/// repeat skipped), never updated or deleted. Its own connection, beside
/// the sessions' one (WAL).
pub struct SqliteEstimates {
    conn: Arc<Mutex<Connection>>,
    /// Times every row (`at`); a fixed one in tests.
    clock: Clock,
}

impl SqliteEstimates {
    /// A store on `conn`, already migrated (`sqlite::open` or
    /// `sqlite::open_in_memory`), timing rows by `clock`.
    pub fn new(conn: Connection, clock: Clock) -> Self {
        Self {
            conn: Arc::new(Mutex::new(conn)),
            clock,
        }
    }

    /// Run `f` on the connection in a thread where blocking is fine, with
    /// "now" from the clock (rounded like everything stored). `&mut`: a
    /// transaction needs it (`record`); the reads are one statement each,
    /// atomic without.
    async fn run<T, F>(&self, f: F) -> Result<T, EstimateError>
    where
        T: Send + 'static,
        F: FnOnce(&mut Connection, Time) -> Result<T, EstimateError> + Send + 'static,
    {
        let conn = Arc::clone(&self.conn);
        let now = to_ms((self.clock)());
        tokio::task::spawn_blocking(move || f(&mut conn.lock().unwrap(), now))
            .await
            .map_err(|e| EstimateError::Backend(e.to_string()))?
    }
}

impl EstimateStore for SqliteEstimates {
    async fn record(&self, row: NewEstimate) -> Result<Option<Recorded>, EstimateError> {
        self.run(move |conn, now| {
            // write lock first (`BEGIN IMMEDIATE`): another process cannot
            // write between our check and our insert
            let tx = write_tx(conn)?;
            if latest_in_chain(&tx, &row)?.is_some_and(|last| row.repeats(&last.estimate)) {
                return Ok(None); // `tx` dropped: rolled back, nothing was written
            }
            let recorded = Recorded {
                id: Uuid::now_v7(),
                estimate: row,
                at: now,
            };
            insert(&tx, &recorded)?;
            tx.commit()?;
            Ok(Some(recorded))
        })
        .await
    }

    async fn last_of(&self, task: NodeId) -> Result<Option<Recorded>, EstimateError> {
        self.run(move |conn, _| Ok(latest(conn, task)?)).await
    }

    async fn of_tasks(&self, tasks: &[NodeId]) -> Result<Vec<Recorded>, EstimateError> {
        if tasks.is_empty() {
            return Ok(Vec::new());
        }
        let tasks = tasks.to_vec(); // `run` needs an owned, `'static` value
        self.run(move |conn, _| {
            // `IN` is a set test: a task asked for twice still gives its rows once.
            // One `?` per task: SQLite takes at most 32766, so more tasks than
            // that are an error (for "every row" add a method, not a longer list)
            let marks = vec!["?"; tasks.len()].join(", ");
            let mut stmt = conn.prepare(&format!(
                "SELECT * FROM estimates WHERE task_id IN ({marks}) ORDER BY at, id"
            ))?;
            let rows = stmt.query_map(params_from_iter(&tasks), recorded_from_row)?;
            Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
        })
        .await
    }
}

/// The latest row of `task` (`id` second: rows of one millisecond still
/// have one "latest").
fn latest(conn: &Connection, task: NodeId) -> rusqlite::Result<Option<Recorded>> {
    conn.query_row(
        "SELECT * FROM estimates WHERE task_id = ?1 ORDER BY at DESC, id DESC LIMIT 1",
        [task],
        recorded_from_row,
    )
    .optional()
}

/// The latest row of `row`'s task in `row`'s chain (`same_chain`; order as
/// `latest`). Reads the task's rows newest first: a task has few.
fn latest_in_chain(conn: &Connection, row: &NewEstimate) -> rusqlite::Result<Option<Recorded>> {
    let mut stmt =
        conn.prepare("SELECT * FROM estimates WHERE task_id = ?1 ORDER BY at DESC, id DESC")?;
    for r in stmt.query_map([row.task], recorded_from_row)? {
        let r = r?;
        if row.same_chain(&r.estimate) {
            return Ok(Some(r));
        }
    }
    Ok(None)
}

/// Store a new row.
fn insert(conn: &Connection, r: &Recorded) -> Result<(), EstimateError> {
    let e = &r.estimate;
    let (at, at_offset) = time_to_sql(r.at);
    let (source, container, tasks) = source_to_sql(e.prior_source);
    conn.execute(
        "INSERT INTO estimates (
             id, task_id, minutes, method, version, done_tasks, open_tasks,
             prior_minutes, reason, at, at_offset,
             prior_source, prior_container, prior_tasks
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
        params![
            r.id.to_string(), // `Uuid` has no `ToSql` (orphan rule); text like sessions
            e.task,
            e.minutes.get(),
            e.method,
            e.version,
            e.done_tasks as i64, // `usize` is no `ToSql` (rusqlite feature `fallible_uint`)
            e.open_tasks as i64,
            e.prior.map(Minutes::get),
            e.reason,
            at,
            at_offset,
            source,
            container,
            tasks,
        ],
    )?;
    Ok(())
}

/// One `estimates` row -> a `Recorded`.
fn recorded_from_row(row: &Row) -> rusqlite::Result<Recorded> {
    let id: String = row.get("id")?;
    let id = id
        .parse::<Uuid>()
        .map_err(|e| rusqlite::Error::FromSqlConversionFailure(0, Type::Text, Box::new(e)))?;
    Ok(Recorded {
        id,
        estimate: NewEstimate {
            task: row.get("task_id")?,
            minutes: Minutes::new(row.get("minutes")?),
            method: row.get("method")?,
            version: row.get("version")?,
            done_tasks: count(row, "done_tasks")?,
            open_tasks: count(row, "open_tasks")?,
            prior: row
                .get::<_, Option<u32>>("prior_minutes")?
                .map(Minutes::new),
            prior_source: source_from_row(row)?,
            reason: row.get("reason")?,
        },
        at: time_from_row(row, "at", "at_offset")?,
    })
}

/// A task count column as `usize`; negative or too large is an error.
fn count(row: &Row, column: &str) -> rusqlite::Result<usize> {
    let n: i64 = row.get(column)?;
    usize::try_from(n)
        .map_err(|e| rusqlite::Error::FromSqlConversionFailure(0, Type::Integer, Box::new(e)))
}

/// A `PriorSource` as its three columns: `prior_source`, `prior_container`,
/// `prior_tasks`. `None` (before #60): all NULL.
fn source_to_sql(s: Option<PriorSource>) -> (Option<&'static str>, Option<NodeId>, Option<i64>) {
    match s {
        None => (None, None, None),
        Some(PriorSource::NoPrior) => (Some("none"), None, None),
        Some(PriorSource::Setting { container }) => (Some("setting"), Some(container), None),
        Some(PriorSource::Parent { container, tasks }) => {
            (Some("parent"), Some(container), Some(tasks as i64)) // `usize` is no `ToSql`
        }
    }
}

/// The three columns -> a `PriorSource`; any other combination (a source
/// without its container, an unknown name, a negative count) is an error.
fn source_from_row(row: &Row) -> rusqlite::Result<Option<PriorSource>> {
    let source: Option<String> = row.get("prior_source")?;
    let container: Option<NodeId> = row.get("prior_container")?;
    let tasks: Option<i64> = row.get("prior_tasks")?;
    let broken = || {
        rusqlite::Error::FromSqlConversionFailure(
            0,
            Type::Text,
            format!("prior source {source:?} / {container:?} / {tasks:?}").into(),
        )
    };
    Ok(match (source.as_deref(), container, tasks) {
        (None, None, None) => None,
        (Some("none"), None, None) => Some(PriorSource::NoPrior),
        (Some("setting"), Some(container), None) => Some(PriorSource::Setting { container }),
        (Some("parent"), Some(container), Some(n)) => Some(PriorSource::Parent {
            container,
            tasks: usize::try_from(n).map_err(|_| broken())?,
        }),
        _ => return Err(broken()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        estimate::store::Reason,
        storage::sqlite,
        test_util::{at, parse_time},
    };

    // The contract (in `mod.rs`) checks the rules every backend keeps; here
    // what only SQLite has: a file, and a format in it.

    fn store() -> SqliteEstimates {
        SqliteEstimates::new(sqlite::open_in_memory().unwrap(), || at(20, 0))
    }

    /// A learned estimate of `task` without a prior, at its first session.
    fn row(task: NodeId) -> NewEstimate {
        NewEstimate {
            task,
            minutes: Minutes::new(185),
            method: "average".into(),
            version: 1,
            done_tasks: 2,
            open_tasks: 1,
            prior: None,
            prior_source: Some(PriorSource::Setting { container: task }),
            reason: Reason::Started,
        }
    }

    /// Two processes (a tmux hook and the TUI) start the same task at once:
    /// both see "first session", both want the same `started` row. One
    /// writes it, the other sees it and skips. Were check and write two
    /// steps, both could write.
    #[tokio::test]
    async fn the_same_row_at_once_is_written_once() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("udo.db");
        let hook = SqliteEstimates::new(sqlite::open(&path).unwrap(), || at(20, 0));
        let tui = SqliteEstimates::new(sqlite::open(&path).unwrap(), || at(20, 0));

        // many rounds: the race is short, each round is one more chance
        for round in 0..100 {
            let task = NodeId::new();

            let (a, b) = tokio::join!(hook.record(row(task)), tui.record(row(task)),);

            let written = [a.unwrap(), b.unwrap()].iter().flatten().count();
            assert_eq!(written, 1, "round {round}");
            let stored = hook.of_tasks(&[task]).await.unwrap();
            assert_eq!(stored.len(), 1, "round {round}");
        }
    }

    /// The written row survives closing and reopening the file, time
    /// offset included; the reopened store's own clock does not touch it.
    #[tokio::test]
    async fn rows_survive_a_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("udo.db");
        let task = NodeId::new();
        let written = {
            let s = SqliteEstimates::new(sqlite::open(&path).unwrap(), || {
                parse_time("2026-10-15T08:30:00-04:00") // another offset than `at`
            });
            s.record(row(task)).await.unwrap().unwrap()
        }; // dropped: connection closed

        let reopened = SqliteEstimates::new(sqlite::open(&path).unwrap(), || at(21, 0));

        let found = reopened.last_of(task).await.unwrap().unwrap();
        assert_eq!(found, written);
        assert_eq!(found.at.offset(), written.at.offset()); // `==` alone ignores it
    }

    /// The columns as stored, readable in the sqlite CLI. Since 0.1.0 the
    /// format must not change: renaming a `Reason` or writing the id as a
    /// blob would pass every round trip, but break existing rows.
    #[tokio::test]
    async fn rows_are_stored_as_readable_text_and_numbers() {
        let s = store();
        let task = NodeId::new();
        let written = s.record(row(task)).await.unwrap().unwrap();

        let conn = s.conn.lock().unwrap();
        let stored: (String, String, u32, String, String, Option<u32>, i64) = conn
            .query_row(
                "SELECT id, task_id, minutes, method, reason, prior_minutes, at FROM estimates",
                [],
                |r| {
                    Ok((
                        r.get(0)?,
                        r.get(1)?,
                        r.get(2)?,
                        r.get(3)?,
                        r.get(4)?,
                        r.get(5)?,
                        r.get(6)?,
                    ))
                },
            )
            .unwrap();

        assert_eq!(
            stored,
            (
                written.id.to_string(),
                task.to_string(),
                185,
                "average".into(),
                "started".into(),
                None, // no prior: NULL
                at(20, 0).timestamp_millis(),
            )
        );
        let source: (String, String, Option<i64>) = conn
            .query_row(
                "SELECT prior_source, prior_container, prior_tasks FROM estimates",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(source, ("setting".into(), task.to_string(), None));
    }

    /// A row from before migration 003 has no source: read as `None`.
    #[tokio::test]
    async fn a_row_from_before_003_has_no_prior_source() {
        let s = store();
        let task = NodeId::new();
        s.record(row(task)).await.unwrap();
        s.conn
            .lock()
            .unwrap()
            .execute(
                "UPDATE estimates SET prior_source = NULL, prior_container = NULL",
                [],
            )
            .unwrap();

        let read = s.last_of(task).await.unwrap().unwrap();

        assert_eq!(read.estimate.prior_source, None);
    }

    /// A broken row (edited by hand, or a bug) is an error when read, never
    /// a panic or a silently wrong value.
    #[tokio::test]
    async fn broken_rows_are_errors_not_panics() {
        for (column, value) in [
            ("id", "'not a uuid'"),
            ("task_id", "''"),
            ("reason", "'Created'"),      // names are lowercase
            ("done_tasks", "-1"),         // a count below zero
            ("prior_minutes", "-5"),      // does not fit `u32`
            ("prior_source", "'pooled'"), // no such source
            ("prior_container", "NULL"),  // a setting without its container
        ] {
            let s = store();
            s.record(row(NodeId::new())).await.unwrap().unwrap();
            let conn = s.conn.lock().unwrap();
            conn.execute(&format!("UPDATE estimates SET {column} = {value}"), [])
                .unwrap();

            // every method reads through `recorded_from_row`
            let read = conn.query_row("SELECT * FROM estimates", [], recorded_from_row);

            assert!(read.is_err(), "{column} = {value} was read: {read:?}");
        }
    }
}
