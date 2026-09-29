//! `Time` in SQL: two integer columns, UTC milliseconds and the offset in
//! minutes. Sub-millisecond parts are dropped; so are offset seconds (only
//! historical zones before ~1900 have them).

use chrono::{DateTime, FixedOffset, SubsecRound};
use rusqlite::{Row, types::Type};

use crate::model::time::Time;

/// `Time` -> (UTC ms, offset in minutes).
pub fn time_to_sql(t: Time) -> (i64, i32) {
    (t.timestamp_millis(), t.offset().local_minus_utc() / 60)
}

/// Read a `Time` from its two columns (`<ms>`, `<offset>`).
pub fn time_from_row(row: &Row, ms: &str, offset: &str) -> rusqlite::Result<Time> {
    let (value, minutes): (i64, i32) = (row.get(ms)?, row.get(offset)?);
    let bad =
        || rusqlite::Error::FromSqlConversionFailure(0, Type::Integer, "time out of range".into());
    // checked: a broken value must be an error, not an overflow panic
    let offset = minutes
        .checked_mul(60)
        .and_then(FixedOffset::east_opt)
        .ok_or_else(bad)?;
    let utc = DateTime::from_timestamp_millis(value).ok_or_else(bad)?;
    Ok(utc.with_timezone(&offset))
}

/// Same for a nullable time (both columns NULL = `None`).
pub fn opt_time_from_row(row: &Row, ms: &str, offset: &str) -> rusqlite::Result<Option<Time>> {
    match row.get::<_, Option<i64>>(ms)? {
        None => Ok(None),
        Some(_) => time_from_row(row, ms, offset).map(Some),
    }
}

/// Rounded to what the database keeps (ms), so what a method returns is
/// what reading returns later.
pub fn to_ms(t: Time) -> Time {
    t.trunc_subsecs(3)
}

#[cfg(test)]
mod tests {
    use chrono::TimeDelta;
    use rusqlite::Connection;

    use super::*;
    use crate::test_util::parse_time;

    /// Read (ms, offset) back as it would come out of two columns.
    fn read(ms: Option<i64>, offset: Option<i32>) -> rusqlite::Result<Option<Time>> {
        let conn = Connection::open_in_memory().unwrap();
        conn.query_row("SELECT ?1 AS ms, ?2 AS off", (ms, offset), |row| {
            opt_time_from_row(row, "ms", "off")
        })
    }

    fn round_trip(t: Time) -> Time {
        let (ms, offset) = time_to_sql(t);
        read(Some(ms), Some(offset)).unwrap().unwrap()
    }

    #[test]
    fn round_trip_keeps_instant_and_offset() {
        for t in [
            parse_time("2026-10-15T14:30:00+02:00"), // summer, Vienna
            parse_time("2026-12-15T14:30:00+01:00"), // winter
            parse_time("2026-10-15T08:30:00-04:00"), // west of UTC
            parse_time("2026-10-15T20:00:00+05:45"), // quarter-hour zone (Nepal)
            parse_time("1969-07-20T20:17:00+00:00"), // before 1970: negative ms
        ] {
            let back = round_trip(t);
            assert_eq!(back, t);
            assert_eq!(back.offset(), t.offset(), "{t}"); // `==` alone ignores it
        }
    }

    #[test]
    fn stored_to_the_millisecond() {
        let t = parse_time("2026-10-15T14:30:00+02:00") + TimeDelta::nanoseconds(123_456_789);

        let back = round_trip(t);

        assert_ne!(back, t); // sub-ms part dropped
        assert_eq!(back, parse_time("2026-10-15T14:30:00.123+02:00"));
    }

    #[test]
    fn null_is_none() {
        assert_eq!(read(None, None).unwrap(), None);
    }

    #[test]
    fn broken_values_are_errors_not_panics() {
        assert!(read(Some(0), Some(i32::MAX)).is_err()); // overflows `* 60`
        assert!(read(Some(0), Some(25 * 60)).is_err()); // offset over 24 h
        assert!(read(Some(i64::MAX), Some(0)).is_err()); // beyond chrono's range
        assert!(read(Some(0), None).is_err()); // a time without its offset
    }
}
