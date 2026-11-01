//! Tracked sessions as intervals: one per session.

use super::Source;
use crate::{
    model::{sessions::Session, time::Time},
    stats::{Interval, SourceKind},
};

/// The sessions the caller got from the store, already in the span and the
/// scope. The engine clips them to the span.
pub struct SessionsSource<'a>(pub &'a [Session]);

impl Source for SessionsSource<'_> {
    fn intervals(&self, _from: Time, _to: Time, now: Time) -> Vec<Interval> {
        self.0
            .iter()
            .map(|s| Interval {
                start: s.start,
                end: s.end.unwrap_or(now),
                task: s.task.clone(),
                session: Some(s.id),
                source: SourceKind::Tracked,
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::{at, session};

    #[test]
    fn a_running_session_ends_at_now() {
        let running = session(at(9, 0), None);

        let intervals = SessionsSource(&[running]).intervals(at(0, 0), at(23, 59), at(10, 30));

        assert_eq!(intervals[0].end, at(10, 30));
    }

    #[test]
    fn a_session_keeps_its_end() {
        let done = session(at(9, 0), Some(at(9, 45)));

        let intervals =
            SessionsSource(std::slice::from_ref(&done)).intervals(at(0, 0), at(23, 59), at(10, 30));

        assert_eq!(intervals[0].start, at(9, 0));
        assert_eq!(intervals[0].end, at(9, 45));
        assert_eq!(intervals[0].session, Some(done.id));
        assert_eq!(intervals[0].source, SourceKind::Tracked);
    }
}
