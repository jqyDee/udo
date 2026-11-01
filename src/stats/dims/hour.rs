//! Hour of the local day, 0-23. Every local hour boundary cuts. A DST day's
//! repeated hour is two occurrences, a skipped one none.

use chrono::{SubsecRound, TimeDelta, TimeZone, Timelike};

use super::Dim;
use crate::{
    model::time::Time,
    stats::{Ctx, KeyPart, interval::Piece},
};

#[derive(Debug, Clone, Copy)]
pub struct Hour;

impl<Tz: TimeZone> Dim<Tz> for Hour {
    fn name(&self) -> &'static str {
        "hour"
    }

    fn cuts(&self, from: Time, to: Time, tz: &Tz) -> Vec<Time> {
        boundaries(from, to, tz)
    }

    fn key(&self, piece: &Piece<'_>, _from: Time, _to: Time, ctx: &Ctx<'_, Tz>) -> KeyPart {
        KeyPart::Hour(local_hour(piece.start, &ctx.tz))
    }

    fn keys_in(&self, from: Time, to: Time, tz: &Tz) -> Option<Vec<(KeyPart, u32)>> {
        if from >= to {
            return Some(Vec::new());
        }
        // Each hour the span touches starts at `from` or at a boundary.
        let mut occurrences = [0u32; 24];
        for start in std::iter::once(from).chain(boundaries(from, to, tz)) {
            occurrences[usize::from(local_hour(start, tz))] += 1;
        }
        Some(
            occurrences
                .iter()
                .enumerate()
                .map(|(hour, &n)| (KeyPart::Hour(hour as u8), n))
                .collect(),
        )
    }
}

/// The local hour of an instant.
fn local_hour<Tz: TimeZone>(t: Time, tz: &Tz) -> u8 {
    u8::try_from(t.with_timezone(tz).hour()).expect("an hour is 0..=23")
}

/// The instants in `(from, to)` where the local hour changes, in order. The
/// first is the next whole local hour after `from`; the rest step by an hour
/// of absolute time, so a DST switch moves the clock, not the boundaries.
fn boundaries<Tz: TimeZone>(from: Time, to: Time, tz: &Tz) -> Vec<Time> {
    let from = from.trunc_subsecs(0);
    let local = from.with_timezone(tz);
    let into_hour = i64::from(local.minute() * 60 + local.second());
    let mut at = from + (TimeDelta::hours(1) - TimeDelta::seconds(into_hour));
    let mut boundaries = Vec::new();
    while at < to {
        boundaries.push(at);
        at += TimeDelta::hours(1);
    }
    boundaries
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use chrono_tz::Europe::Berlin;
    use serde_json::json;

    use super::*;
    use crate::{
        model::{
            id::NodeId,
            sessions::{SessionId, TaskRef},
        },
        stats::{
            DimSpec, Interval, Key, MeasureSpec, Query, Row, Rows, SourceKind,
            dims::calendar::Unit, run,
        },
        test_util::{parse_time, tree_with},
    };

    /// A tracked interval on one task, in a session of its own.
    fn iv(start: &str, end: &str) -> Interval {
        Interval {
            start: parse_time(start),
            end: parse_time(end),
            task: TaskRef {
                id: NodeId::new(),
                name: "a".into(),
                description: String::new(),
                container_dir: PathBuf::from("/uni"),
                container_id: NodeId::new(),
            },
            session: Some(SessionId::new()),
            source: SourceKind::Tracked,
        }
    }

    /// `run` over `from`..`to` with the given dimensions, in Berlin.
    fn rows(dims: Vec<DimSpec>, from: &str, to: &str, intervals: &[Interval]) -> Rows {
        let query = Query {
            from: parse_time(from),
            to: parse_time(to),
            dims,
            measures: vec![MeasureSpec::Minutes, MeasureSpec::Sessions],
            include_removed_tasks: false,
        };
        let tree = tree_with(vec![]);
        let ctx = Ctx {
            tree: &tree,
            scope: &[],
            tz: Berlin,
        };
        run(&query, intervals, &ctx)
    }

    /// The row of hour `hour`.
    fn row(rows: &Rows, hour: u8) -> &Row {
        let key = Key(vec![KeyPart::Hour(hour)]);
        rows.rows
            .iter()
            .find(|row| row.key == key)
            .expect("every hour is listed")
    }

    #[test]
    fn hour_splits_session() {
        let intervals = [iv("2026-10-26T09:40:00+01:00", "2026-10-26T10:20:00+01:00")];
        let rows = rows(
            vec![DimSpec::Hour],
            "2026-10-26T00:00:00+01:00",
            "2026-10-27T00:00:00+01:00",
            &intervals,
        );
        assert_eq!(row(&rows, 9).values["minutes"], json!(20));
        assert_eq!(row(&rows, 10).values["minutes"], json!(20));
        assert_eq!(row(&rows, 11).values["minutes"], json!(0));
        assert_eq!(rows.total.values["minutes"], json!(40));
    }

    #[test]
    fn hour_occurrences_on_dst_days() {
        // Fall back: the 02:00 hour happens twice.
        let fall = rows(
            vec![DimSpec::Hour],
            "2026-10-25T00:00:00+02:00",
            "2026-10-26T00:00:00+01:00",
            &[],
        );
        assert_eq!(fall.rows.len(), 24);
        assert_eq!(row(&fall, 2).occurrences, 2);
        assert!(
            fall.rows
                .iter()
                .filter(|row| row.key != Key(vec![KeyPart::Hour(2)]))
                .all(|row| row.occurrences == 1)
        );

        // Spring forward: there is no 02:00 hour, but it stays listed.
        let spring = rows(
            vec![DimSpec::Hour],
            "2026-03-29T00:00:00+01:00",
            "2026-03-30T00:00:00+02:00",
            &[],
        );
        assert_eq!(spring.rows.len(), 24);
        assert_eq!(row(&spring, 2).occurrences, 0);
        assert_eq!(row(&spring, 2).values["minutes"], json!(0));
        assert!(
            spring
                .rows
                .iter()
                .filter(|row| row.key != Key(vec![KeyPart::Hour(2)]))
                .all(|row| row.occurrences == 1)
        );
    }

    #[test]
    fn day_by_hour_counts_jointly() {
        // Each hour of the day once, except the repeated 02:00 on fall back.
        let rows = rows(
            vec![
                DimSpec::Calendar {
                    unit: Unit::Day,
                    every: 1,
                },
                DimSpec::Hour,
            ],
            "2026-10-25T00:00:00+02:00",
            "2026-10-26T00:00:00+01:00",
            &[],
        );
        assert_eq!(rows.rows.len(), 24);
        for row in &rows.rows {
            let expected = if row.key.0[1] == KeyPart::Hour(2) {
                2
            } else {
                1
            };
            assert_eq!(row.occurrences, expected, "{:?}", row.key);
        }
    }
}
