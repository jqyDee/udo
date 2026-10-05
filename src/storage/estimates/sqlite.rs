//! Estimate rows in `udo.db` (table `estimates`, migration 002).

use std::sync::{Arc, Mutex};

use rusqlite::{Connection, OptionalExtension, Row, params, params_from_iter, types::Type};
use uuid::Uuid;

use crate::{
    model::{
        estimate_store::{EstimateError, EstimateStore, NewEstimate, Recorded},
        id::NodeId,
        time::{Clock, Minutes, Time},
    },
    storage::time::{time_from_row, time_to_sql, to_ms},
};

/// The estimate history in `udo.db`. Append-only: rows are inserted, never
/// updated or deleted. Its own connection, beside the sessions' one (WAL).
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
    /// "now" from the clock (rounded like everything stored). One statement
    /// per method: no transaction needed, each is atomic on its own.
    async fn run<T, F>(&self, f: F) -> Result<T, EstimateError>
    where
        T: Send + 'static,
        F: FnOnce(&Connection, Time) -> Result<T, EstimateError> + Send + 'static,
    {
        let conn = Arc::clone(&self.conn);
        let now = to_ms((self.clock)());
        tokio::task::spawn_blocking(move || f(&conn.lock().unwrap(), now))
            .await
            .map_err(|e| EstimateError::Backend(e.to_string()))?
    }
}

impl EstimateStore for SqliteEstimates {
    async fn record(&self, row: NewEstimate) -> Result<Recorded, EstimateError> {
        self.run(move |conn, now| {
            let recorded = Recorded {
                id: Uuid::now_v7(),
                estimate: row,
                at: now,
            };
            insert(conn, &recorded)?;
            Ok(recorded)
        })
        .await
    }

    async fn last_of(&self, task: NodeId) -> Result<Option<Recorded>, EstimateError> {
        self.run(move |conn, _| {
            // `id` second: rows of one millisecond still have one "latest"
            let found = conn
                .query_row(
                    "SELECT * FROM estimates WHERE task_id = ?1
                     ORDER BY at DESC, id DESC LIMIT 1",
                    [task],
                    recorded_from_row,
                )
                .optional()?;
            Ok(found)
        })
        .await
    }

    async fn of_tasks(&self, tasks: &[NodeId]) -> Result<Vec<Recorded>, EstimateError> {
        if tasks.is_empty() {
            return Ok(Vec::new());
        }
        let tasks = tasks.to_vec(); // `run` needs an owned, `'static` value
        self.run(move |conn, _| {
            // `IN` is a set test: a task asked for twice still gives its rows once
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

/// Store a new row.
fn insert(conn: &Connection, r: &Recorded) -> Result<(), EstimateError> {
    let e = &r.estimate;
    let (at, at_offset) = time_to_sql(r.at);
    conn.execute(
        "INSERT INTO estimates (
             id, task_id, minutes, method, version, done_tasks, open_tasks,
             prior_minutes, reason, at, at_offset
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::sqlite;

    super::super::contract::estimate_store_contract!(|clock| {
        SqliteEstimates::new(sqlite::open_in_memory().unwrap(), clock)
    });
}
