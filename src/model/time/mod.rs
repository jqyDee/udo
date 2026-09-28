//! Time values used by settings and tasks. Both are written as text in
//! files, forms and the CLI.
//!
//! - `Time`: a point in time with its offset (`now`, `local_to_fixed`)
//! - `duration`: `Minutes`, e.g. `1h30`, `45m`
//! - `deadline`: `DeadlineRule`, e.g. `fri 22:00`, `+7d 23:59`

mod deadline;
mod duration;

use chrono::{DateTime, FixedOffset, Local, NaiveDateTime, TimeZone};
pub use deadline::DeadlineRule;
pub use duration::Minutes;

/// A point in time: the instant plus the local offset when it was recorded.
/// Always shown in the current local time (`with_timezone(&Local)`); the
/// stored offset is data, not display. Note: `==` compares the instant only.
pub type Time = DateTime<FixedOffset>;

/// Where a store gets "now" for bookkeeping times (created, edited,
/// deleted). The app passes `now`; tests pass a fixed time.
pub type Clock = fn() -> Time;

/// Now, with the local offset. The one way udo gets the current time.
pub fn now() -> Time {
    Local::now().fixed_offset()
}

/// Local form time -> `Time` for storing. None if that time doesn't exist
/// (skipped by a DST switch).
pub fn local_to_fixed(local: NaiveDateTime) -> Option<Time> {
    Local
        .from_local_datetime(&local)
        .earliest()
        .map(|t| t.fixed_offset())
}

#[cfg(test)]
mod tests {
    use serde::{Deserialize, Serialize};

    use super::*;

    #[derive(Serialize, Deserialize)]
    struct Row {
        at: Time,
    }

    #[test]
    fn offset_survives_a_toml_round_trip() {
        // `==` ignores the offset, so compare it on its own
        let at = DateTime::parse_from_rfc3339("2026-10-15T14:30:00+02:00").unwrap();
        let text = toml::to_string(&Row { at }).unwrap();
        let back: Row = toml::from_str(&text).unwrap();
        assert_eq!(back.at, at);
        assert_eq!(back.at.offset(), at.offset());
    }

    #[test]
    fn old_utc_values_still_load() {
        // files written before the switch hold `…Z`
        let back: Row = toml::from_str(r#"at = "2026-10-15T12:30:00Z""#).unwrap();
        assert_eq!(back.at.offset().local_minus_utc(), 0);
    }
}
