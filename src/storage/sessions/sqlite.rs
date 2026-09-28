use std::sync::{Arc, Mutex};

use rusqlite::{
    Connection, ErrorCode, OptionalExtension, Row, ToSql, Transaction, TransactionBehavior, params,
    params_from_iter,
};

use crate::{
    model::{
        sessions::{
            Session, SessionError, SessionId, SessionPatch, SessionQuery, SessionSource,
            SessionStore, TaskRef,
        },
        time::{Clock, Time},
    },
    storage::time::{opt_time_from_row, time_from_row, time_to_sql, to_ms},
};

pub struct SqliteSessions {
    conn: Arc<Mutex<Connection>>,
    clock: Clock,
}

impl SqliteSessions {
    pub fn new(conn: Connection, clock: Clock) -> Self {
        Self {
            conn: Arc::new(Mutex::new(conn)),
            clock,
        }
    }

    /// Run `f` on the connection in a thread where blocking is fine, with
    /// "now" from the clock (rounded like everything stored).
    async fn run<T, F>(&self, f: F) -> Result<T, SessionError>
    where
        T: Send + 'static,
        F: FnOnce(&mut Connection, Time) -> Result<T, SessionError> + Send + 'static,
    {
        let conn = Arc::clone(&self.conn);
        let now = to_ms((self.clock)());
        tokio::task::spawn_blocking(move || f(&mut conn.lock().unwrap(), now))
            .await
            .map_err(|e| SessionError::Backend(e.to_string()))?
    }
}

impl SessionStore for SqliteSessions {
    async fn start(
        &self,
        task: TaskRef,
        source: SessionSource,
        at: Time,
    ) -> Result<Session, SessionError> {
        let at = to_ms(at); // round like the database, before any comparison
        self.run(move |conn, now| start(conn, now, task, source, at))
            .await
    }

    async fn stop(&self, at: Time) -> Result<Option<Session>, SessionError> {
        let at = to_ms(at);
        self.run(move |conn, now| stop(conn, now, at)).await
    }

    async fn running(&self) -> Result<Option<Session>, SessionError> {
        self.run(|conn, _| {
            let tx = conn.transaction()?; // reading only: no write lock needed
            Ok(running(&tx)?)
        })
        .await
    }

    async fn query(&self, q: &SessionQuery) -> Result<Vec<Session>, SessionError> {
        let q = q.clone(); // `run` needs an owned, `'static` value, not a borrow
        self.run(move |conn, _| query(conn, q)).await
    }

    async fn add(&self, task: TaskRef, start: Time, end: Time) -> Result<Session, SessionError> {
        todo!()
    }

    async fn split(
        &self,
        id: SessionId,
        at: Time,
    ) -> Result<Option<(Session, Session)>, SessionError> {
        todo!()
    }

    async fn cut(&self, id: SessionId, from: Time, to: Time) -> Result<Vec<Session>, SessionError> {
        todo!()
    }

    async fn edit(&self, id: SessionId, patch: SessionPatch) -> Result<(), SessionError> {
        todo!()
    }

    async fn delete(&self, id: SessionId) -> Result<(), SessionError> {
        todo!()
    }
}

/// Every column of a session, plus `edited_at`: the time of its latest
/// entry in the edit log (none = never edited).
const SELECT: &str = "
    SELECT s.*, e.at AS edited_at, e.at_offset AS edited_offset
    FROM sessions s
    LEFT JOIN session_edits e ON e.id = (
        SELECT id FROM session_edits
        WHERE session_id = s.id
        ORDER BY at DESC, id DESC
        LIMIT 1
    )";

/// One row of `SELECT` -> a `Session`.
fn session_from_row(row: &Row) -> rusqlite::Result<Session> {
    Ok(Session {
        id: row.get("id")?,
        task: TaskRef {
            id: row.get("task_id")?,
            name: row.get("task_name")?,
            description: row.get("task_description")?,
            container_path: row.get("container_path")?,
        },
        start: time_from_row(row, "started_at", "start_offset")?,
        end: opt_time_from_row(row, "ended_at", "end_offset")?,
        source: row.get("source")?,
        created_at: time_from_row(row, "created_at", "created_offset")?,
        edited_at: opt_time_from_row(row, "edited_at", "edited_offset")?,
        deleted_at: opt_time_from_row(row, "deleted_at", "deleted_offset")?,
    })
}

/// The running session, if any
fn running(tx: &Transaction) -> rusqlite::Result<Option<Session>> {
    tx.query_row(
        &format!("{SELECT} WHERE s.ended_at IS NULL AND s.deleted_at IS NULL"),
        [],
        session_from_row,
    )
    .optional()
}

/// A transaction that takes the write lock right away (`BEGIN IMMEDIATE`):
/// checks and changes happen as one step, no other process in between.
fn write_tx(conn: &mut Connection) -> rusqlite::Result<Transaction<'_>> {
    conn.transaction_with_behavior(TransactionBehavior::Immediate)
}

/// Store a new session row. (`edited_at` is not a column: it comes from
/// the edit log.)
fn insert(tx: &Transaction, s: &Session) -> Result<(), SessionError> {
    let (start, start_offset) = time_to_sql(s.start);
    let (end, end_offset) = s.end.map(time_to_sql).unzip();
    let (created, created_offset) = time_to_sql(s.created_at);
    let (deleted, deleted_offset) = s.deleted_at.map(time_to_sql).unzip();

    tx.execute(
        "INSERT INTO sessions (
             id, task_id, task_name, task_description, container_path,
             started_at, start_offset, ended_at, end_offset, source,
             created_at, created_offset, deleted_at, deleted_offset
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
        params![
            s.id,
            s.task.id,
            s.task.name,
            s.task.description,
            s.task.container_path,
            start,
            start_offset,
            end,
            end_offset,
            s.source,
            created,
            created_offset,
            deleted,
            deleted_offset,
        ],
    )
    .map_err(already_running)?;
    Ok(())
}

/// The `one_running` index refused a second running session (two
/// processes racing): say so, instead of a raw database error.
fn already_running(e: rusqlite::Error) -> SessionError {
    match e {
        rusqlite::Error::SqliteFailure(f, _) if f.code == ErrorCode::ConstraintViolation => {
            SessionError::AlreadyRunning
        }
        e => e.into(),
    }
}

/// What stopping the running session did.
enum Stopped {
    Nothing,
    Stopped(Session),
    /// `at` was before its start: soft-deleted. Commit, then report
    /// `EndBeforeStart` (an early `return Err` would roll the delete back).
    ClockError,
}

/// Stop the running session at `at`, if any. Shared by `start` and `stop`.
fn stop_running(tx: &Transaction, now: Time, at: Time) -> Result<Stopped, SessionError> {
    let Some(mut session) = running(tx)? else {
        return Ok(Stopped::Nothing);
    };
    if at < session.start {
        let (deleted, offset) = time_to_sql(now);
        tx.execute(
            "UPDATE sessions SET deleted_at = ?1, deleted_offset = ?2 WHERE id = ?3",
            params![deleted, offset, session.id],
        )?;
        return Ok(Stopped::ClockError);
    }
    let (end, offset) = time_to_sql(at);
    tx.execute(
        "UPDATE sessions SET ended_at = ?1, end_offset = ?2 WHERE id = ?3",
        params![end, offset, session.id],
    )?;
    session.end = Some(at);
    Ok(Stopped::Stopped(session))
}

fn start(
    conn: &mut Connection,
    now: Time,
    task: TaskRef,
    source: SessionSource,
    at: Time,
) -> Result<Session, SessionError> {
    let tx = write_tx(conn)?;
    if let Some(running) = running(&tx)?
        && running.task.id == task.id
    {
        return Ok(running); // same task: no-op, nothing to commit
    }
    if let Stopped::ClockError = stop_running(&tx, now, at)? {
        tx.commit()?; // keep the soft delete
        return Err(SessionError::EndBeforeStart);
    }
    let session = Session {
        id: SessionId::new(),
        task,
        start: at,
        end: None,
        source,
        created_at: now,
        edited_at: None,
        deleted_at: None,
    };
    insert(&tx, &session)?;
    tx.commit()?;
    Ok(session)
}

fn stop(conn: &mut Connection, now: Time, at: Time) -> Result<Option<Session>, SessionError> {
    let tx = write_tx(conn)?;
    let stopped = stop_running(&tx, now, at)?;
    tx.commit()?; // for a clock error too: the soft delete must stay
    match stopped {
        Stopped::Nothing => Ok(None),
        Stopped::Stopped(session) => Ok(Some(session)),
        Stopped::ClockError => Err(SessionError::EndBeforeStart),
    }
}

fn query(conn: &mut Connection, q: SessionQuery) -> Result<Vec<Session>, SessionError> {
    let mut sql = format!("{SELECT} WHERE 1 = 1");
    let mut args: Vec<Box<dyn ToSql>> = Vec::new();

    if !q.include_deleted {
        sql += " AND s.deleted_at IS NULL";
    }

    if let Some(tasks) = q.tasks {
        if tasks.is_empty() {
            return Ok(Vec::new()); // here maybe throw a new error, rather than just returning nothing!
        }
        let marks = vec!["?"; tasks.len()].join(", ");
        sql += &format!(" AND s.task_id IN ({marks})");
        for id in tasks {
            args.push(Box::new(id));
        }
    }
    // `[start, end)` intersects `[from, to)`; see `intersects` in `rules.rs`
    if let Some(to) = q.to {
        sql += " AND s.started_at < ?";
        args.push(Box::new(time_to_sql(to).0));
    }
    if let Some(from) = q.from {
        sql += " AND (s.ended_at IS NULL OR ? < s.ended_at)";
        args.push(Box::new(time_to_sql(from).0));
    }
    sql += " ORDER BY s.started_at";

    let tx = conn.transaction()?; // reading only: deferred is enough
    let mut stmt = tx.prepare(&sql)?;
    let rows = stmt.query_map(params_from_iter(&args), session_from_row)?;
    let found = rows.collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::sqlite;

    super::super::contract::store_contract!(|clock: Clock| {
        SqliteSessions::new(sqlite::open_in_memory().unwrap(), clock)
    });
}
