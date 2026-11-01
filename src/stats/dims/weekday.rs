//! Weekday, local: Monday first. Every local midnight cuts. Occurrences are
//! the dates with that weekday the span touches.

use chrono::{Datelike, TimeDelta, TimeZone};

use super::{
    Dim,
    calendar::{day_of, midnight},
};
use crate::{
    model::time::Time,
    stats::{Ctx, KeyPart, interval::Piece},
};

/// Monday first, the order of the keys.
const WEEK: [chrono::Weekday; 7] = [
    chrono::Weekday::Mon,
    chrono::Weekday::Tue,
    chrono::Weekday::Wed,
    chrono::Weekday::Thu,
    chrono::Weekday::Fri,
    chrono::Weekday::Sat,
    chrono::Weekday::Sun,
];

#[derive(Debug, Clone, Copy)]
pub struct Weekday;

impl<Tz: TimeZone> Dim<Tz> for Weekday {
    fn name(&self) -> &'static str {
        "weekday"
    }

    fn cuts(&self, from: Time, to: Time, tz: &Tz) -> Vec<Time> {
        let mut cuts = Vec::new();
        let mut next = day_of(from, tz).succ_opt();
        while let Some(day) = next {
            let at = midnight(day, tz);
            if at >= to {
                break;
            }
            cuts.push(at);
            next = day.succ_opt();
        }
        cuts
    }

    fn key(&self, piece: &Piece<'_>, _from: Time, _to: Time, ctx: &Ctx<'_, Tz>) -> KeyPart {
        KeyPart::Weekday(day_of(piece.start, &ctx.tz).weekday())
    }

    fn keys_in(&self, from: Time, to: Time, tz: &Tz) -> Option<Vec<(KeyPart, u32)>> {
        if from >= to {
            return Some(Vec::new());
        }
        // The local dates run on from the one of `from` to the one of the
        // last instant before `to`.
        let last = day_of(to - TimeDelta::nanoseconds(1), tz);
        let mut occurrences = [0u32; 7];
        for day in day_of(from, tz).iter_days().take_while(|day| *day <= last) {
            occurrences[day.weekday().num_days_from_monday() as usize] += 1;
        }
        Some(
            WEEK.iter()
                .map(|&day| {
                    (
                        KeyPart::Weekday(day),
                        occurrences[day.num_days_from_monday() as usize],
                    )
                })
                .collect(),
        )
    }
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
        stats::{DimSpec, Interval, Key, MeasureSpec, Query, Rows, SourceKind, run},
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

    #[test]
    fn weekday_occurrences_in_october_2026() {
        // 1 Oct 2026 is a Thursday, so Thu, Fri and Sat come five times.
        let rows = rows(
            vec![DimSpec::Weekday],
            "2026-10-01T00:00:00+02:00",
            "2026-11-01T00:00:00+01:00",
            &[],
        );
        let occurrences: Vec<u32> = rows.rows.iter().map(|row| row.occurrences).collect();
        assert_eq!(occurrences, vec![4, 4, 4, 5, 5, 5, 4]);
    }

    #[test]
    fn weekday_order_mon_first() {
        let rows = rows(
            vec![DimSpec::Weekday],
            "2026-10-26T00:00:00+01:00",
            "2026-11-02T00:00:00+01:00",
            &[],
        );
        let keys: Vec<Key> = rows.rows.iter().map(|row| row.key.clone()).collect();
        let week = [
            chrono::Weekday::Mon,
            chrono::Weekday::Tue,
            chrono::Weekday::Wed,
            chrono::Weekday::Thu,
            chrono::Weekday::Fri,
            chrono::Weekday::Sat,
            chrono::Weekday::Sun,
        ];
        let expected: Vec<Key> = week
            .into_iter()
            .map(|day| Key(vec![KeyPart::Weekday(day)]))
            .collect();
        assert_eq!(keys, expected);
    }

    #[test]
    fn weekday_by_hour_counts_jointly() {
        // Oct 2026 has five Thursdays and four Mondays, each with one 09:00.
        let rows = rows(
            vec![DimSpec::Weekday, DimSpec::Hour],
            "2026-10-01T00:00:00+02:00",
            "2026-11-01T00:00:00+01:00",
            &[],
        );
        let cell = |day: chrono::Weekday, hour: u8| {
            rows.rows
                .iter()
                .find(|row| row.key == Key(vec![KeyPart::Weekday(day), KeyPart::Hour(hour)]))
                .expect("every cell is listed")
        };
        assert_eq!(cell(chrono::Weekday::Mon, 9).occurrences, 4);
        assert_eq!(cell(chrono::Weekday::Thu, 9).occurrences, 5);
    }

    #[test]
    fn weekday_by_hour_cross_product() {
        let rows = rows(
            vec![DimSpec::Weekday, DimSpec::Hour],
            "2026-10-26T00:00:00+01:00",
            "2026-11-02T00:00:00+01:00",
            &[iv("2026-10-27T09:00:00+01:00", "2026-10-27T10:00:00+01:00")],
        );
        assert_eq!(rows.rows.len(), 7 * 24);
        assert_eq!(rows.total.values["minutes"], json!(60));
        let cell = |day: chrono::Weekday, hour: u8| {
            rows.rows
                .iter()
                .find(|row| row.key == Key(vec![KeyPart::Weekday(day), KeyPart::Hour(hour)]))
                .expect("every cell is listed")
        };
        // The session is Tuesday 09:00 to 10:00.
        assert_eq!(cell(chrono::Weekday::Tue, 9).values["minutes"], json!(60));
        assert_eq!(cell(chrono::Weekday::Tue, 10).values["minutes"], json!(0));
    }
}
