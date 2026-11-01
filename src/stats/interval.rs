//! Intervals: the engine's input, one stretch of time from one source, and
//! the pieces they are cut into.

use chrono::{SubsecRound, Timelike};
use serde::{Deserialize, Serialize};

use crate::model::{
    sessions::{SessionId, TaskRef},
    time::Time,
};

/// A stretch of time from one source. The engine knows nothing else about
/// where it came from.
#[derive(Debug, Clone, PartialEq)]
pub struct Interval {
    pub start: Time,
    /// A running session: `now`.
    pub end: Time,
    pub task: TaskRef,
    /// For counting sessions; `None` for other sources later.
    pub session: Option<SessionId>,
    pub source: SourceKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    Tracked,
}

/// An interval cut to the span and at every boundary: lies inside exactly
/// one key of every dimension.
#[derive(Debug, Clone, Copy)]
pub struct Piece<'a> {
    pub interval: &'a Interval,
    pub start: Time,
    pub end: Time,
}

impl Piece<'_> {
    /// Length in whole seconds. Sums are taken in seconds (see `minutes_of`).
    pub fn seconds(&self) -> i64 {
        (self.end - self.start).num_seconds()
    }
}

/// Clips every interval to `[from, to)`, drops the empty ones and cuts the
/// rest at each of `cuts` (sorted, deduplicated) that lies strictly inside.
/// Endpoints are truncated to whole seconds first (the cuts are whole
/// seconds too), so the pieces add up to the uncut length exactly.
pub fn cut<'a>(intervals: &'a [Interval], from: Time, to: Time, cuts: &[Time]) -> Vec<Piece<'a>> {
    debug_assert!(
        cuts.iter().all(|cut| cut.nanosecond() == 0),
        "cuts must be whole seconds: exact totals rely on it"
    );
    let from = from.trunc_subsecs(0);
    let to = to.trunc_subsecs(0);
    let mut pieces = Vec::new();
    for interval in intervals {
        let start = interval.start.trunc_subsecs(0).max(from);
        let end = interval.end.trunc_subsecs(0).min(to);
        if end <= start {
            continue;
        }
        let lo = cuts.partition_point(|c| *c <= start);
        let hi = cuts.partition_point(|c| *c < end);
        let mut piece_start = start;
        for &cut in &cuts[lo..hi] {
            pieces.push(Piece {
                interval,
                start: piece_start,
                end: cut,
            });
            piece_start = cut;
        }
        pieces.push(Piece {
            interval,
            start: piece_start,
            end,
        });
    }
    pieces
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::parse_time;

    #[test]
    #[should_panic(expected = "whole seconds")]
    fn sub_second_cut_is_refused() {
        let cut_at = parse_time("2026-10-15T10:00:00.500+02:00");
        cut(
            &[],
            parse_time("2026-10-15T00:00:00+02:00"),
            cut_at,
            &[cut_at],
        );
    }
}
