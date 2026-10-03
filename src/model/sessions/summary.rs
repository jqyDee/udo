//! `TimeSummary`: the numbers over a list of sessions (duration, count,
//! running), shared by `udo show` and the TUI details.

use std::fmt;

use crate::model::{
    sessions::Session,
    time::{Minutes, Time},
};

/// Time recorded on a node, as of `now`: what the details rows and
/// `udo show` display.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimeSummary {
    /// Sum of the sessions' whole minutes (a running one up to `now`).
    pub duration: Minutes,
    /// How many sessions.
    pub sessions: usize,
    /// One of them is running.
    pub running: bool,
}

impl TimeSummary {
    /// The summary of `sessions` (all of one node), a running one timed up
    /// to `now`. Each session is rounded down first, so the rows of a list
    /// add up to `duration`.
    pub fn of(sessions: &[Session], now: Time) -> Self {
        Self {
            duration: Minutes::new(sessions.iter().map(|s| s.duration(now).get()).sum()),
            sessions: sessions.len(),
            running: sessions.iter().any(|s| s.end.is_none()),
        }
    }
}

/// `1h12 in 3 sessions, running` (one: `session`; `, running` only then).
impl fmt::Display for TimeSummary {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let noun = if self.sessions == 1 {
            "session"
        } else {
            "sessions"
        };
        let running = if self.running { ", running" } else { "" };
        write!(f, "{} in {} {noun}{running}", self.duration, self.sessions)
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::{
        model::{
            id::NodeId,
            sessions::{Owner, SessionId, SessionSource, TaskRef},
        },
        test_util::{at, parse_time},
    };

    fn session(start: Time, end: Option<Time>) -> Session {
        Session {
            id: SessionId::new(),
            task: TaskRef {
                id: NodeId::new(),
                name: "lab 3".into(),
                description: String::new(),
                container_dir: PathBuf::from("/uni"),
                container_id: NodeId::new(),
            },
            start,
            end,
            source: SessionSource::Manual,
            owner: Owner::manual(),
            created_at: start,
            edited_at: None,
            deleted_at: None,
        }
    }

    #[test]
    fn no_sessions_is_zero() {
        let summary = TimeSummary::of(&[], at(12, 0));

        assert_eq!(
            summary,
            TimeSummary {
                duration: Minutes::new(0),
                sessions: 0,
                running: false,
            }
        );
    }

    #[test]
    fn finished_sessions_add_up() {
        let sessions = [
            session(at(9, 0), Some(at(10, 30))),
            session(at(14, 0), Some(at(14, 45))),
        ];

        let summary = TimeSummary::of(&sessions, at(20, 0)); // `now` does not matter

        assert_eq!(summary.duration, Minutes::new(135));
        assert_eq!(summary.sessions, 2);
        assert!(!summary.running);
    }

    #[test]
    fn a_running_session_counts_up_to_now() {
        let sessions = [session(at(9, 0), Some(at(10, 0))), session(at(14, 0), None)];

        let summary = TimeSummary::of(&sessions, at(14, 12));

        assert_eq!(summary.duration, Minutes::new(72));
        assert!(summary.running);
    }

    /// Each session is rounded down first, so the rows add up to the sum.
    #[test]
    fn each_session_is_rounded_down_before_the_sum() {
        let t = |s: &str| parse_time(&format!("2026-10-15T{s}+02:00"));
        let sessions = [
            session(t("09:00:00"), Some(t("09:01:30"))),
            session(t("10:00:00"), Some(t("10:01:30"))),
        ];

        let summary = TimeSummary::of(&sessions, at(20, 0));

        assert_eq!(summary.duration, Minutes::new(2)); // 1 + 1, not 3
    }

    #[test]
    fn text_counts_sessions_in_words() {
        let summary = |minutes, sessions, running| TimeSummary {
            duration: Minutes::new(minutes),
            sessions,
            running,
        };

        assert_eq!(summary(0, 0, false).to_string(), "0m in 0 sessions");
        assert_eq!(summary(45, 1, false).to_string(), "45m in 1 session");
        assert_eq!(
            summary(72, 3, true).to_string(),
            "1h12 in 3 sessions, running"
        );
    }
}
