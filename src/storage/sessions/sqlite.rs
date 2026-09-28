use std::sync::{Arc, Mutex};

use rusqlite::{
    Connection, ErrorCode, OptionalExtension, Row, ToSql, Transaction, TransactionBehavior, params,
    params_from_iter,
};
use uuid::Uuid;

use crate::{
    model::{
        sessions::{
            Session, SessionError, SessionId, SessionPatch, SessionQuery, SessionSource,
            SessionStore, TaskRef,
        },
        time::{Clock, Time},
    },
    storage::{
        edit_kind::EditKind,
        sessions::rules::{Cut, Split, check_span, plan_cut, plan_edit, plan_split},
        time::{opt_time_from_row, time_from_row, time_to_sql, to_ms},
    },
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
        let (start, end) = (to_ms(start), to_ms(end));
        self.run(move |conn, now| add(conn, now, task, start, end))
            .await
    }

    async fn split(
        &self,
        id: SessionId,
        at: Time,
    ) -> Result<Option<(Session, Session)>, SessionError> {
        let at = to_ms(at);
        self.run(move |conn, now| split(conn, now, id, at)).await
    }

    async fn cut(&self, id: SessionId, from: Time, to: Time) -> Result<Vec<Session>, SessionError> {
        let (from, to) = (to_ms(from), to_ms(to));
        self.run(move |conn, now| cut(conn, now, id, from, to))
            .await
    }

    async fn edit(&self, id: SessionId, patch: SessionPatch) -> Result<(), SessionError> {
        let patch = SessionPatch {
            start: patch.start.map(to_ms),
            end: patch.end.map(to_ms),
        };
        self.run(move |conn, now| edit(conn, now, id, patch)).await
    }

    async fn delete(&self, id: SessionId) -> Result<(), SessionError> {
        self.run(move |conn, now| delete(conn, now, id)).await
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
    Ended(Session),
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
    Ok(Stopped::Ended(session))
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
        Stopped::Ended(session) => Ok(Some(session)),
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

/// Does `[start, end)` overlap a visible session other than `except`?
/// `end: None` = open-ended (running).
///
/// * `start` time already rounded to ms
/// * `end` time already rounded to ms, if exists
fn overlaps(
    tx: &Transaction,
    start: Time,       //
    end: Option<Time>, // time already rounded to ms
    except: Option<SessionId>,
) -> Result<bool, SessionError> {
    let start = time_to_sql(start).0;
    let end = end.map(|end| time_to_sql(end).0);
    let found = tx.query_row(
        "SELECT EXISTS (
             SELECT 1 FROM sessions
             WHERE deleted_at IS NULL
               AND id IS NOT ?3
               AND (?2 IS NULL OR started_at < ?2)
               AND (ended_at IS NULL OR ?1 < ended_at)
         )",
        params![start, end, except],
        |row| row.get(0),
    )?;
    Ok(found)
}

/// A visible session; unknown or deleted: `NotFound`.
fn find(tx: &Transaction, id: SessionId) -> Result<Session, SessionError> {
    tx.query_row(
        &format!("{SELECT} WHERE s.id = ?1 AND s.deleted_at IS NULL"),
        [id],
        session_from_row,
    )
    .optional()?
    .ok_or(SessionError::NotFound)
}

/// Store new start / end of a session (end `None` = running)
///
/// * `start` time already rounded to ms
/// * `end` time already rounded to ms, if exists
fn set_times(
    tx: &Transaction,
    id: SessionId,
    start: Time,
    end: Option<Time>,
) -> Result<(), SessionError> {
    let (start, start_offset) = time_to_sql(start);
    let (end, end_offset) = end.map(time_to_sql).unzip();
    tx.execute(
        "UPDATE sessions
         SET started_at = ?1, start_offset = ?2, ended_at = ?3, end_offset = ?4
         WHERE id = ?5",
        params![start, start_offset, end, end_offset, id],
    )?;
    Ok(())
}

/// Start and end of a session (end `None` = running), for the edit log.
type Span = (Time, Option<Time>);

/// One `session_edits` row: `kind` changed `session`, from `old` to `new`.
/// `old: None` = the session is new (a split / cut piece); `new: None` =
/// it is gone (delete).
fn log(
    tx: &Transaction,
    session: SessionId,
    now: Time,
    kind: EditKind,
    old: Option<Span>,
    new: Option<Span>,
) -> Result<(), SessionError> {
    // a span -> its four columns: ms and offset of start and of end
    let columns = |span: Option<Span>| {
        let (start, end) = span.map_or((None, None), |(start, end)| (Some(start), end));
        let (start, start_offset) = start.map(time_to_sql).unzip();
        let (end, end_offset) = end.map(time_to_sql).unzip();
        (start, start_offset, end, end_offset)
    };
    let (old_start, old_start_offset, old_end, old_end_offset) = columns(old);
    let (new_start, new_start_offset, new_end, new_end_offset) = columns(new);
    let (at, at_offset) = time_to_sql(now);
    tx.execute(
        "INSERT INTO session_edits (
             id, session_id, at, at_offset, kind,
             old_start, old_start_offset, old_end, old_end_offset,
             new_start, new_start_offset, new_end, new_end_offset
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
        params![
            Uuid::now_v7().to_string(),
            session,
            at,
            at_offset,
            kind,
            old_start,
            old_start_offset,
            old_end,
            old_end_offset,
            new_start,
            new_start_offset,
            new_end,
            new_end_offset,
        ],
    )?;
    Ok(())
}

fn add(
    conn: &mut Connection,
    now: Time,
    task: TaskRef,
    start: Time,
    end: Time,
) -> Result<Session, SessionError> {
    check_span(start, end)?;
    let tx = write_tx(conn)?;
    if overlaps(&tx, start, Some(end), None)? {
        return Err(SessionError::Overlap);
    }
    let session = Session {
        id: SessionId::new(),
        task,
        start,
        end: Some(end),
        source: SessionSource::Manual,
        created_at: now,
        edited_at: None,
        deleted_at: None,
    };
    insert(&tx, &session)?;
    tx.commit()?;
    Ok(session)
}

fn edit(
    conn: &mut Connection,
    now: Time,
    id: SessionId,
    patch: SessionPatch,
) -> Result<(), SessionError> {
    let tx = write_tx(conn)?;
    let session = find(&tx, id)?;
    let (start, end) = plan_edit(session.start, session.end, &patch)?;
    if overlaps(&tx, start, end, Some(id))? {
        return Err(SessionError::Overlap);
    }
    set_times(&tx, id, start, end)?;
    log(
        &tx,
        id,
        now,
        EditKind::Edit,
        Some((session.start, session.end)),
        Some((start, end)),
    )?;
    tx.commit()?;
    Ok(())
}

fn delete(conn: &mut Connection, now: Time, id: SessionId) -> Result<(), SessionError> {
    let tx = write_tx(conn)?;
    let session = find(&tx, id)?;
    if session.end.is_none() && now >= session.start {
        set_times(&tx, id, session.start, Some(now))?; // running: stopped first
    }
    let (deleted, offset) = time_to_sql(now);
    tx.execute(
        "UPDATE sessions SET deleted_at = ?1, deleted_offset = ?2 WHERE id = ?3",
        params![deleted, offset, id],
    )?;
    log(&tx, id, now, EditKind::Delete, Some((session.start, session.end)), None)?;
    tx.commit()?;
    Ok(())
}

fn split(
    conn: &mut Connection,
    now: Time,
    id: SessionId,
    at: Time,
) -> Result<Option<(Session, Session)>, SessionError> {
    let tx = write_tx(conn)?;
    let session = find(&tx, id)?;
    let Some(Split { at, end }) = plan_split(session.start, session.end, at)? else {
        return Ok(None); // on an edge: nothing changes, nothing to commit
    };
    let old = (session.start, session.end);
    let first = Session {
        end: Some(at),
        edited_at: Some(now),
        ..session
    };
    let second = Session {
        id: SessionId::new(),
        start: at,
        end: Some(end),
        ..first.clone()
    };

    set_times(&tx, first.id, first.start, first.end)?;
    log(&tx, first.id, now, EditKind::Split, Some(old), Some((first.start, first.end)))?;
    insert(&tx, &second)?;
    log(&tx, second.id, now, EditKind::Split, None, Some((second.start, second.end)))?;
    tx.commit()?;
    Ok(Some((first, second)))
}

fn cut(
    conn: &mut Connection,
    now: Time,
    id: SessionId,
    from: Time,
    to: Time,
) -> Result<Vec<Session>, SessionError> {
    let tx = write_tx(conn)?;
    let session = find(&tx, id)?;
    let old = (session.start, session.end);
    let plan = plan_cut(session.start, session.end, from, to)?;
    let edited = Session {
        edited_at: Some(now),
        ..session
    };
    let left = match plan {
        Cut::TrimStart(start) => vec![Session { start, ..edited }],
        Cut::TrimEnd(end) => vec![Session {
            end: Some(end),
            ..edited
        }],
        Cut::Gap { from, to } => vec![
            Session {
                end: Some(from),
                ..edited.clone()
            },
            Session {
                id: SessionId::new(),
                start: to,
                ..edited // the old end: a running session keeps running
            },
        ],
    };

    // the first piece first: if the session was running, it gets its end
    // here, *before* the second piece (the running one now) is inserted
    let first = &left[0];
    set_times(&tx, first.id, first.start, first.end)?;
    log(&tx, first.id, now, EditKind::Cut, Some(old), Some((first.start, first.end)))?;
    if let Some(second) = left.get(1) {
        insert(&tx, second)?;
        log(&tx, second.id, now, EditKind::Cut, None, Some((second.start, second.end)))?;
    }
    tx.commit()?;
    Ok(left)
}

#[cfg(test)]
mod tests {
    use chrono::DateTime;

    use super::*;
    use crate::{model::id::NodeId, storage::sqlite};

    super::super::contract::store_contract!(|clock: Clock| {
        SqliteSessions::new(sqlite::open_in_memory().unwrap(), clock)
    });

    // The edit log is SQLite's own (the contract cannot see it): what gets
    // written there is tested here.

    fn time(rfc3339: &str) -> Time {
        DateTime::parse_from_rfc3339(rfc3339).unwrap()
    }

    /// 2026-10-15 at `hh:mm`, +02:00.
    fn at(hh_mm: &str) -> Time {
        time(&format!("2026-10-15T{hh_mm}:00+02:00"))
    }

    fn store() -> SqliteSessions {
        SqliteSessions::new(sqlite::open_in_memory().unwrap(), || at("20:00"))
    }

    fn task() -> TaskRef {
        TaskRef {
            id: NodeId::new(),
            name: "lab 3".into(),
            description: String::new(),
            container_path: "uni/cs".into(),
        }
    }

    fn patch_end(end: Time) -> SessionPatch {
        SessionPatch {
            end: Some(end),
            ..Default::default()
        }
    }

    /// One `session_edits` row, as stored.
    #[derive(Debug)]
    struct Logged {
        session: SessionId,
        kind: EditKind,
        old: Option<Span>,
        new: Option<Span>,
    }

    /// The whole edit log, oldest first.
    fn edits(s: &SqliteSessions) -> Vec<Logged> {
        let conn = s.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT * FROM session_edits ORDER BY rowid")
            .unwrap();
        stmt.query_map([], |row| {
            // four columns -> a span (no start = no span)
            let span = |start: &str, end: &str| -> rusqlite::Result<Option<Span>> {
                let start = opt_time_from_row(row, start, &format!("{start}_offset"))?;
                let end = opt_time_from_row(row, end, &format!("{end}_offset"))?;
                Ok(start.map(|start| (start, end)))
            };
            Ok(Logged {
                session: row.get("session_id")?,
                kind: row.get("kind")?,
                old: span("old_start", "old_end")?,
                new: span("new_start", "new_end")?,
            })
        })
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap()
    }

    #[tokio::test]
    async fn recording_is_not_logged() {
        let s = store();
        s.add(task(), at("09:00"), at("10:00")).await.unwrap();
        s.start(task(), SessionSource::Manual, at("14:00"))
            .await
            .unwrap();
        s.stop(at("15:00")).await.unwrap();
        s.start(task(), SessionSource::Manual, at("16:00"))
            .await
            .unwrap();
        assert!(s.stop(at("13:00")).await.is_err()); // a clock error, not a correction

        assert!(edits(&s).is_empty());
    }

    #[tokio::test]
    async fn every_correction_logs_one_row_per_piece_with_its_kind() {
        let s = store();
        let a = s.add(task(), at("09:00"), at("17:00")).await.unwrap();

        s.edit(a.id, patch_end(at("16:00"))).await.unwrap();
        let (a, b) = s.split(a.id, at("12:00")).await.unwrap().unwrap();
        let c = s.cut(b.id, at("13:00"), at("14:00")).await.unwrap()[1].clone();
        s.delete(c.id).await.unwrap();

        let logged: Vec<_> = edits(&s).iter().map(|e| (e.session, e.kind)).collect();
        assert_eq!(
            logged,
            vec![
                (a.id, EditKind::Edit),
                (a.id, EditKind::Split),
                (b.id, EditKind::Split),
                (b.id, EditKind::Cut),
                (c.id, EditKind::Cut),
                (c.id, EditKind::Delete),
            ]
        );
    }

    #[tokio::test]
    async fn log_keeps_old_and_new_times_with_their_offsets() {
        let s = store();
        let a = s.add(task(), at("09:00"), at("11:00")).await.unwrap();
        let new_end = time("2026-10-15T06:00:00-04:00"); // 12:00 here, another offset

        s.edit(a.id, patch_end(new_end)).await.unwrap();
        let (_, second) = s.split(a.id, at("10:00")).await.unwrap().unwrap();
        s.delete(second.id).await.unwrap();

        let log = edits(&s);
        // edit: 9–11 -> 9–12
        assert_eq!(log[0].old, Some((at("09:00"), Some(at("11:00")))));
        assert_eq!(log[0].new, Some((at("09:00"), Some(new_end))));
        let logged_end = log[0].new.unwrap().1.unwrap();
        assert_eq!(logged_end.offset(), new_end.offset()); // `==` alone ignores it
        // split: the new piece did not exist before
        assert_eq!(log[2].session, second.id);
        assert_eq!(log[2].old, None);
        assert_eq!(log[2].new, Some((at("10:00"), Some(new_end))));
        // delete: gone afterwards
        assert_eq!(log[3].old, Some((at("10:00"), Some(new_end))));
        assert_eq!(log[3].new, None);
    }
}
