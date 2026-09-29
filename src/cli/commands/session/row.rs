//! How the session commands show sessions: one `SessionRow` each, and
//! `SessionDone` for what a correction left.

use std::fmt;

use chrono::Local;
use serde::Serialize;

use super::id::short_id;
use crate::{
    cli::{report::Report, resolve::path_text},
    core::Core,
    model::{
        sessions::Session,
        time::{Minutes, Time},
        tree::Tree,
    },
};

/// One session as the session commands show it.
#[derive(Serialize)]
pub struct SessionRow {
    /// The short form (`short_id`).
    pub id: String,
    pub full_id: String,
    pub task: String,
    /// The task's path; `None`: the task was removed.
    pub path: Option<String>,
    pub start: Time,
    /// `None`: running.
    pub end: Option<Time>,
    pub minutes: u32,
}

impl SessionRow {
    /// `session` timed up to `now`, its task looked up in `tree`.
    pub fn of(session: &Session, tree: &Tree, now: Time) -> Self {
        let path = tree
            .rows()
            .into_iter()
            .find(|r| r.node.id() == session.task.id)
            .map(|r| path_text(tree, &r.path));
        Self {
            id: short_id(session.id),
            full_id: session.id.to_string(),
            task: session.task.name.clone(),
            path,
            start: session.start,
            end: session.end,
            minutes: session.duration(now).get(),
        }
    }

    /// `4f9e2c  thu 2026-10-15  14:00-15:12  1h12  uni/cs/lab 3`
    pub fn line(&self) -> String {
        let start = self.start.with_timezone(&Local);
        let end = match self.end {
            Some(end) => end.with_timezone(&Local).format("%H:%M").to_string(),
            None => "now".into(),
        };
        let range = format!("{}-{end}", start.format("%H:%M"));
        let place = match &self.path {
            Some(path) => path.clone(),
            None => format!("{} (deleted)", self.task),
        };
        format!(
            "{}  {} {}  {range:<11}  {:<5} {place}",
            self.id,
            start.format("%a").to_string().to_lowercase(),
            start.format("%Y-%m-%d"),
            Minutes::new(self.minutes).to_string(),
        )
    }
}

/// What a correction left: the sessions it made or changed.
#[derive(Serialize)]
pub struct SessionDone {
    /// `added`, `edited`, `split`, `cut`, `removed`
    pub action: String,
    pub sessions: Vec<SessionRow>,
}

impl SessionDone {
    pub fn of(action: &str, sessions: &[Session], core: &Core, now: Time) -> Self {
        Self {
            action: action.into(),
            sessions: sessions
                .iter()
                .map(|s| SessionRow::of(s, core.tree(), now))
                .collect(),
        }
    }
}

impl fmt::Display for SessionDone {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:", self.action)?;
        for row in &self.sessions {
            write!(f, "\n{}", row.line())?;
        }
        Ok(())
    }
}

impl Report for SessionDone {}
