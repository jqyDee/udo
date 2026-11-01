//! Calendar buckets: day, week, month or year in the query's zone. Bucket
//! starts are local midnights; a DST switch that skips midnight starts the
//! day at its first instant. The edge buckets are cut to the span.

use chrono::{Datelike, Months, NaiveDate, NaiveTime, TimeDelta, TimeZone};
use serde::{Deserialize, Serialize};

use super::Dim;
use crate::{
    model::time::Time,
    stats::{Ctx, KeyPart, interval::Piece},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Unit {
    Day,
    Week,
    Month,
    Year,
}

/// `every` units per bucket. Buckets count from the unit that holds the
/// span's `from`. `every` 0 (possible from JSON) counts as 1.
#[derive(Debug, Clone, Copy)]
pub struct Calendar {
    pub unit: Unit,
    pub every: u32,
}

impl Calendar {
    fn anchor<Tz: TimeZone>(&self, from: Time, tz: &Tz) -> NaiveDate {
        unit_start(self.unit, day_of(from, tz))
    }

    /// The start day of the bucket that holds `day`, buckets counting from
    /// `anchor`.
    fn bucket_of(&self, anchor: NaiveDate, day: NaiveDate) -> NaiveDate {
        let units = units_between(self.unit, anchor, unit_start(self.unit, day));
        let step = self.step();
        advance(self.unit, anchor, units / step * step).expect("a day inside the span is in range")
    }

    /// The bucket that starts on `start`, cut to `[from, to)`.
    fn part<Tz: TimeZone>(&self, start: NaiveDate, from: Time, to: Time, tz: &Tz) -> KeyPart {
        let end = advance(self.unit, start, self.step()).map_or(to, |next| midnight(next, tz));
        KeyPart::Calendar {
            start: midnight(start, tz).max(from),
            end: end.min(to),
        }
    }

    fn step(&self) -> i64 {
        i64::from(self.every.max(1))
    }
}

impl<Tz: TimeZone> Dim<Tz> for Calendar {
    fn name(&self) -> &'static str {
        match self.unit {
            Unit::Day => "day",
            Unit::Week => "week",
            Unit::Month => "month",
            Unit::Year => "year",
        }
    }

    fn cuts(&self, from: Time, to: Time, tz: &Tz) -> Vec<Time> {
        let mut cuts = Vec::new();
        let anchor = self.anchor(from, tz);
        let mut next = advance(
            self.unit,
            self.bucket_of(anchor, day_of(from, tz)),
            self.step(),
        );
        while let Some(day) = next {
            let at = midnight(day, tz);
            if at >= to {
                break;
            }
            if at > from {
                cuts.push(at);
            }
            next = advance(self.unit, day, self.step());
        }
        cuts
    }

    fn key(&self, piece: &Piece<'_>, from: Time, to: Time, ctx: &Ctx<'_, Tz>) -> KeyPart {
        let anchor = self.anchor(from, &ctx.tz);
        let start = self.bucket_of(anchor, day_of(piece.start, &ctx.tz));
        self.part(start, from, to, &ctx.tz)
    }

    fn keys_in(&self, from: Time, to: Time, tz: &Tz) -> Option<Vec<(KeyPart, u32)>> {
        let mut keys = Vec::new();
        if from >= to {
            return Some(keys);
        }
        let anchor = self.anchor(from, tz);
        let mut next = Some(self.bucket_of(anchor, day_of(from, tz)));
        while let Some(day) = next {
            if midnight(day, tz) >= to {
                break;
            }
            keys.push((self.part(day, from, to, tz), 1));
            next = advance(self.unit, day, self.step());
        }
        Some(keys)
    }
}

/// The local day of an instant.
pub(super) fn day_of<Tz: TimeZone>(t: Time, tz: &Tz) -> NaiveDate {
    t.with_timezone(tz).date_naive()
}

/// The first day of the `unit` that holds `day`.
fn unit_start(unit: Unit, day: NaiveDate) -> NaiveDate {
    match unit {
        Unit::Day => day,
        Unit::Week => day - TimeDelta::days(i64::from(day.weekday().num_days_from_monday())),
        Unit::Month => day.with_day(1).expect("every month has a 1st"),
        Unit::Year => day.with_ordinal(1).expect("every year has a 1st"),
    }
}

/// Whole units from `from` to `to`, both unit starts.
fn units_between(unit: Unit, from: NaiveDate, to: NaiveDate) -> i64 {
    match unit {
        Unit::Day => (to - from).num_days(),
        Unit::Week => (to - from).num_days() / 7,
        Unit::Month => {
            i64::from(to.year() - from.year()) * 12 + i64::from(to.month())
                - i64::from(from.month())
        }
        Unit::Year => i64::from(to.year() - from.year()),
    }
}

/// `units` units after `day`, a unit start. None past the end of the calendar.
fn advance(unit: Unit, day: NaiveDate, units: i64) -> Option<NaiveDate> {
    match unit {
        Unit::Day => day.checked_add_signed(TimeDelta::days(units)),
        Unit::Week => day.checked_add_signed(TimeDelta::weeks(units)),
        Unit::Month => day.checked_add_months(Months::new(u32::try_from(units).ok()?)),
        Unit::Year => {
            let months = u32::try_from(units.checked_mul(12)?).ok()?;
            day.checked_add_months(Months::new(months))
        }
    }
}

/// The first instant of `day` in `tz`: local midnight, or the first instant
/// after it when a DST switch skips midnight.
pub(crate) fn midnight<Tz: TimeZone>(day: NaiveDate, tz: &Tz) -> Time {
    let start = day.and_time(NaiveTime::MIN);
    (0..=24 * 60)
        .find_map(|minutes| {
            tz.from_local_datetime(&(start + TimeDelta::minutes(minutes)))
                .earliest()
        })
        .expect("every day has a first instant")
        .fixed_offset()
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
        stats::{DimSpec, Interval, MeasureSpec, Query, Rows, SourceKind, run},
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

    /// `run` over `from`..`to` with one calendar dimension, in Berlin.
    fn rows(unit: Unit, every: u32, from: &str, to: &str, intervals: &[Interval]) -> Rows {
        let query = Query {
            from: parse_time(from),
            to: parse_time(to),
            dims: vec![DimSpec::Calendar { unit, every }],
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

    fn bucket(start: &str, end: &str) -> KeyPart {
        KeyPart::Calendar {
            start: parse_time(start),
            end: parse_time(end),
        }
    }

    /// The bucket of each row, in display order.
    fn keys(rows: &Rows) -> Vec<KeyPart> {
        rows.rows.iter().map(|row| row.key.0[0].clone()).collect()
    }

    #[test]
    fn split_at_midnight() {
        let intervals = [iv("2026-10-15T23:30:00+02:00", "2026-10-16T00:30:00+02:00")];
        let rows = rows(
            Unit::Day,
            1,
            "2026-10-15T00:00:00+02:00",
            "2026-10-17T00:00:00+02:00",
            &intervals,
        );
        assert_eq!(
            keys(&rows),
            vec![
                bucket("2026-10-15T00:00:00+02:00", "2026-10-16T00:00:00+02:00"),
                bucket("2026-10-16T00:00:00+02:00", "2026-10-17T00:00:00+02:00"),
            ]
        );
        assert!(rows.rows.iter().all(|r| r.values["minutes"] == json!(30)));
        assert_eq!(rows.total.values["minutes"], json!(60));
    }

    #[test]
    fn week_starts_monday() {
        // Wed 14 Oct to Sat 31 Oct, cut at both edges.
        let rows = rows(
            Unit::Week,
            1,
            "2026-10-14T00:00:00+02:00",
            "2026-10-31T00:00:00+01:00",
            &[],
        );
        assert_eq!(
            keys(&rows),
            vec![
                bucket("2026-10-14T00:00:00+02:00", "2026-10-19T00:00:00+02:00"),
                bucket("2026-10-19T00:00:00+02:00", "2026-10-26T00:00:00+01:00"),
                bucket("2026-10-26T00:00:00+01:00", "2026-10-31T00:00:00+01:00"),
            ]
        );
    }

    #[test]
    fn month_and_year() {
        let months = rows(
            Unit::Month,
            1,
            "2026-10-15T00:00:00+02:00",
            "2026-12-01T00:00:00+01:00",
            &[],
        );
        assert_eq!(
            keys(&months),
            vec![
                bucket("2026-10-15T00:00:00+02:00", "2026-11-01T00:00:00+01:00"),
                bucket("2026-11-01T00:00:00+01:00", "2026-12-01T00:00:00+01:00"),
            ]
        );
        let years = rows(
            Unit::Year,
            1,
            "2025-06-01T00:00:00+02:00",
            "2027-01-01T00:00:00+01:00",
            &[],
        );
        assert_eq!(
            keys(&years),
            vec![
                bucket("2025-06-01T00:00:00+02:00", "2026-01-01T00:00:00+01:00"),
                bucket("2026-01-01T00:00:00+01:00", "2027-01-01T00:00:00+01:00"),
            ]
        );
    }

    #[test]
    fn six_months_aligned_to_from() {
        let rows = rows(
            Unit::Month,
            6,
            "2026-01-01T00:00:00+01:00",
            "2027-01-01T00:00:00+01:00",
            &[],
        );
        assert_eq!(
            keys(&rows),
            vec![
                bucket("2026-01-01T00:00:00+01:00", "2026-07-01T00:00:00+02:00"),
                bucket("2026-07-01T00:00:00+02:00", "2027-01-01T00:00:00+01:00"),
            ]
        );
    }

    #[test]
    fn every_zero_counts_as_one() {
        let rows = rows(
            Unit::Month,
            0,
            "2026-01-01T00:00:00+01:00",
            "2026-07-01T00:00:00+02:00",
            &[],
        );
        assert_eq!(rows.rows.len(), 6);
    }

    #[test]
    fn calendar_day_on_dst_days() {
        // Spring forward: 23 h. Fall back: 25 h.
        let spring = rows(
            Unit::Day,
            1,
            "2026-03-29T00:00:00+01:00",
            "2026-03-30T00:00:00+02:00",
            &[iv("2026-03-29T00:00:00+01:00", "2026-03-30T00:00:00+02:00")],
        );
        assert_eq!(spring.rows.len(), 1);
        assert_eq!(spring.rows[0].seconds, 23 * 3600);
        assert_eq!(spring.rows[0].values["minutes"], json!(1380));

        let fall = rows(
            Unit::Day,
            1,
            "2026-10-25T00:00:00+02:00",
            "2026-10-26T00:00:00+01:00",
            &[iv("2026-10-25T00:00:00+02:00", "2026-10-26T00:00:00+01:00")],
        );
        assert_eq!(fall.rows.len(), 1);
        assert_eq!(fall.rows[0].seconds, 25 * 3600);
        assert_eq!(fall.rows[0].values["minutes"], json!(1500));
    }

    #[test]
    fn future_buckets_are_zero() {
        // Data only on the first day; the span runs on into the future.
        let intervals = [iv("2026-10-29T10:00:00+01:00", "2026-10-29T11:00:00+01:00")];
        let rows = rows(
            Unit::Day,
            1,
            "2026-10-29T00:00:00+01:00",
            "2026-11-02T00:00:00+01:00",
            &intervals,
        );
        let minutes: Vec<_> = rows
            .rows
            .iter()
            .map(|r| r.values["minutes"].clone())
            .collect();
        assert_eq!(minutes, vec![json!(60), json!(0), json!(0), json!(0)]);
        assert!(rows.rows.iter().all(|r| r.occurrences == 1));
    }
}
