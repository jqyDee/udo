//! Date form input: a local date + time, edited one segment at a time.

use std::ops::Range;

use chrono::{Local, Months, NaiveDateTime, TimeDelta};

/// Display format of a date field. Fixed width and ASCII only, so byte
/// ranges == char ranges (see `Segment::range`).
pub const DATE_FMT: &str = "%Y-%m-%d %H:%M";

/// Local date + time, edited one segment at a time (←/→ pick, ↑/↓ change).
/// Local, not UTC: converted only on submit (`App::submit_form`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DateInput {
    pub value: NaiveDateTime,
    pub segment: Segment,
}

/// One editable part of a `DATE_FMT` string.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Segment {
    Year,
    Month,
    Day,
    Hour,
    Minute,
}

impl Segment {
    /// Segment to the right.
    pub fn next(self) -> Self {
        match self {
            Self::Year => Self::Month,
            Self::Month => Self::Day,
            Self::Day => Self::Hour,
            Self::Hour => Self::Minute,
            Self::Minute => Self::Year,
        }
    }

    /// Segment to the left.
    pub fn prev(self) -> Self {
        match self {
            Self::Minute => Self::Hour,
            Self::Hour => Self::Day,
            Self::Day => Self::Month,
            Self::Month => Self::Year,
            Self::Year => Self::Minute,
        }
    }

    /// Where the segment sits in a `DATE_FMT` string.
    pub fn range(self) -> Range<usize> {
        match self {
            Self::Year => 0..4,
            Self::Month => 5..7,
            Self::Day => 8..10,
            Self::Hour => 11..13,
            Self::Minute => 14..16,
        }
    }
}

impl DateInput {
    /// Starts on `Segment::Day`.
    pub fn new(value: NaiveDateTime) -> Self {
        Self {
            value,
            segment: Segment::Day,
        }
    }

    pub fn next_segment(&mut self) {
        self.segment = self.segment.next();
    }

    pub fn prev_segment(&mut self) {
        self.segment = self.segment.prev();
    }

    /// +1 / -1 on the current segment (minutes: ±5).
    /// Year/month: `checked_add_months` / `checked_sub_months` (clamps the
    /// day, Jan 31 + 1 month = Feb 28/29). Day/hour/minute:
    /// `checked_add_signed(TimeDelta)` (carries, 23:55 + 5 min = next day
    /// 00:00). Always `checked_*`, never `+`: `+` panics on overflow, here
    /// overflow -> unchanged.
    pub fn step(&mut self, up: bool) {
        let sign: i64 = if up { 1 } else { -1 };

        let stepped = match self.segment {
            Segment::Year => self.shift_months(12, up),
            Segment::Month => self.shift_months(1, up),
            Segment::Day => self.value.checked_add_signed(TimeDelta::days(sign)),
            Segment::Hour => self.value.checked_add_signed(TimeDelta::hours(sign)),
            Segment::Minute => self.value.checked_add_signed(TimeDelta::minutes(sign * 5)),
        };

        self.value = stepped.unwrap_or(self.value);
    }

    fn shift_months(&self, count: u32, up: bool) -> Option<NaiveDateTime> {
        if up {
            self.value.checked_add_months(Months::new(count))
        } else {
            self.value.checked_sub_months(Months::new(count))
        }
    }

    /// Date -> today, time kept (`t` key).
    pub fn set_today(&mut self) {
        self.value = Local::now().date_naive().and_time(self.value.time());
    }

    pub fn display(&self) -> String {
        self.value.format(DATE_FMT).to_string()
    }
}

#[cfg(test)]
mod tests {
    use chrono::NaiveDate;

    use super::*;

    // --------------- Segment Tests ---------------

    const SEGMENTS: [Segment; 5] = [
        Segment::Year,
        Segment::Month,
        Segment::Day,
        Segment::Hour,
        Segment::Minute,
    ];

    #[test]
    fn segment_next_goes_left_to_right() {
        assert_eq!(Segment::Year.next(), Segment::Month);
        assert_eq!(Segment::Month.next(), Segment::Day);
        assert_eq!(Segment::Day.next(), Segment::Hour);
        assert_eq!(Segment::Hour.next(), Segment::Minute);
    }

    #[test]
    fn segment_next_wraps_from_minute_to_year() {
        assert_eq!(Segment::Minute.next(), Segment::Year);
    }

    #[test]
    fn segment_prev_goes_right_to_left() {
        assert_eq!(Segment::Minute.prev(), Segment::Hour);
        assert_eq!(Segment::Hour.prev(), Segment::Day);
        assert_eq!(Segment::Day.prev(), Segment::Month);
        assert_eq!(Segment::Month.prev(), Segment::Year);
    }

    #[test]
    fn segment_prev_wraps_from_year_to_minute() {
        assert_eq!(Segment::Year.prev(), Segment::Minute);
    }

    #[test]
    fn segment_prev_undoes_next() {
        for s in SEGMENTS {
            assert_eq!(s.next().prev(), s, "{s:?}");
            assert_eq!(s.prev().next(), s, "{s:?}");
        }
    }

    #[test]
    fn segment_range_slices_formatted_date() {
        let date = NaiveDate::from_ymd_opt(2026, 3, 7)
            .unwrap()
            .and_hms_opt(9, 5, 0)
            .unwrap();
        // real DATE_FMT output, zero padding included
        let s = date.format(DATE_FMT).to_string();
        assert_eq!(s, "2026-03-07 09:05");

        assert_eq!(&s[Segment::Year.range()], "2026");
        assert_eq!(&s[Segment::Month.range()], "03");
        assert_eq!(&s[Segment::Day.range()], "07");
        assert_eq!(&s[Segment::Hour.range()], "09");
        assert_eq!(&s[Segment::Minute.range()], "05");
    }

    #[test]
    fn segment_ranges_do_not_overlap_and_stay_in_bounds() {
        let len = "2026-03-07 09:05".len();
        for pair in SEGMENTS.windows(2) {
            let (a, b) = (pair[0].range(), pair[1].range());
            assert!(a.end < b.start, "{:?} and {:?} overlap", pair[0], pair[1]);
        }
        assert!(Segment::Minute.range().end <= len);
    }

    // --------------- DateInput Tests ---------------

    fn dt(y: i32, mo: u32, d: u32, h: u32, mi: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(y, mo, d)
            .unwrap()
            .and_hms_opt(h, mi, 0)
            .unwrap()
    }

    /// `value` stepped once on `segment`.
    fn stepped(value: NaiveDateTime, segment: Segment, up: bool) -> NaiveDateTime {
        let mut d = DateInput { value, segment };
        d.step(up);
        assert_eq!(d.segment, segment, "step must not change the segment");
        d.value
    }

    #[test]
    fn date_input_starts_on_day() {
        let d = DateInput::new(dt(2026, 10, 15, 14, 30));
        assert_eq!(d.segment, Segment::Day);
        assert_eq!(d.value, dt(2026, 10, 15, 14, 30));
    }

    #[test]
    fn date_input_display_uses_date_fmt() {
        let d = DateInput::new(dt(2026, 3, 7, 9, 5));
        assert_eq!(d.display(), "2026-03-07 09:05");
    }

    #[test]
    fn date_input_segment_moves_both_ways() {
        let mut d = DateInput::new(dt(2026, 10, 15, 14, 30)); // on Day
        d.next_segment();
        assert_eq!(d.segment, Segment::Hour);
        d.prev_segment();
        d.prev_segment();
        assert_eq!(d.segment, Segment::Month);
    }

    #[test]
    fn step_each_segment_up_and_down() {
        let v = dt(2026, 6, 15, 12, 30);
        assert_eq!(stepped(v, Segment::Year, true), dt(2027, 6, 15, 12, 30));
        assert_eq!(stepped(v, Segment::Year, false), dt(2025, 6, 15, 12, 30));
        assert_eq!(stepped(v, Segment::Month, true), dt(2026, 7, 15, 12, 30));
        assert_eq!(stepped(v, Segment::Month, false), dt(2026, 5, 15, 12, 30));
        assert_eq!(stepped(v, Segment::Day, true), dt(2026, 6, 16, 12, 30));
        assert_eq!(stepped(v, Segment::Day, false), dt(2026, 6, 14, 12, 30));
        assert_eq!(stepped(v, Segment::Hour, true), dt(2026, 6, 15, 13, 30));
        assert_eq!(stepped(v, Segment::Hour, false), dt(2026, 6, 15, 11, 30));
        assert_eq!(stepped(v, Segment::Minute, true), dt(2026, 6, 15, 12, 35));
        assert_eq!(stepped(v, Segment::Minute, false), dt(2026, 6, 15, 12, 25));
    }

    #[test]
    fn step_day_carries_into_next_month_and_year() {
        assert_eq!(
            stepped(dt(2026, 1, 31, 12, 0), Segment::Day, true),
            dt(2026, 2, 1, 12, 0)
        );
        assert_eq!(
            stepped(dt(2026, 12, 31, 12, 0), Segment::Day, true),
            dt(2027, 1, 1, 12, 0)
        );
    }

    #[test]
    fn step_month_clamps_to_month_end() {
        // leap year / normal year
        assert_eq!(
            stepped(dt(2028, 1, 31, 12, 0), Segment::Month, true),
            dt(2028, 2, 29, 12, 0)
        );
        assert_eq!(
            stepped(dt(2027, 1, 31, 12, 0), Segment::Month, true),
            dt(2027, 2, 28, 12, 0)
        );
        // down as well
        assert_eq!(
            stepped(dt(2026, 3, 31, 12, 0), Segment::Month, false),
            dt(2026, 2, 28, 12, 0)
        );
    }

    #[test]
    fn step_month_carries_across_year() {
        assert_eq!(
            stepped(dt(2026, 12, 15, 12, 0), Segment::Month, true),
            dt(2027, 1, 15, 12, 0)
        );
        assert_eq!(
            stepped(dt(2026, 1, 15, 12, 0), Segment::Month, false),
            dt(2025, 12, 15, 12, 0)
        );
    }

    #[test]
    fn step_year_from_leap_day_clamps() {
        assert_eq!(
            stepped(dt(2028, 2, 29, 12, 0), Segment::Year, true),
            dt(2029, 2, 28, 12, 0)
        );
    }

    #[test]
    fn step_minute_carries_past_midnight() {
        assert_eq!(
            stepped(dt(2026, 10, 15, 23, 55), Segment::Minute, true),
            dt(2026, 10, 16, 0, 0)
        );
    }

    #[test]
    fn step_hour_carries_back_past_midnight() {
        assert_eq!(
            stepped(dt(2026, 10, 15, 0, 30), Segment::Hour, false),
            dt(2026, 10, 14, 23, 30)
        );
    }

    #[test]
    fn step_at_end_of_calendar_leaves_value_unchanged() {
        let max = NaiveDateTime::MAX;
        for s in SEGMENTS {
            assert_eq!(stepped(max, s, true), max, "{s:?} up at MAX");
        }
        let min = NaiveDateTime::MIN;
        for s in SEGMENTS {
            assert_eq!(stepped(min, s, false), min, "{s:?} down at MIN");
        }
    }

    #[test]
    fn set_today_keeps_time_and_segment() {
        let mut d = DateInput {
            value: dt(2000, 1, 1, 9, 45),
            segment: Segment::Hour,
        };

        let before = Local::now().date_naive();
        d.set_today();
        let after = Local::now().date_naive();

        // before/after: don't flake if the test runs across midnight
        assert!(d.value.date() == before || d.value.date() == after);
        assert_eq!(d.value.time(), dt(2000, 1, 1, 9, 45).time());
        assert_eq!(d.segment, Segment::Hour);
    }
}
