//! clap value parsers: text from the command line -> typed values.

use chrono::NaiveDateTime;

use crate::{DATE_FMT, model::time::DeadlineRule};

/// `--due`: a rule relative to now, or a fixed local date.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Due {
    /// `fri 22:00`, `+7d 23:59` (like the `default_deadline` setting).
    Rule(DeadlineRule),
    /// `2026-10-20 09:00`, local time.
    At(NaiveDateTime),
}

impl Due {
    /// The local date and time this means, seen from `now`.
    pub fn after(self, now: NaiveDateTime) -> NaiveDateTime {
        match self {
            Self::Rule(rule) => rule.next_after(now),
            Self::At(at) => at,
        }
    }
}

/// Parse `--due`: a date in `DATE_FMT` first, else a deadline rule.
pub fn due(input: &str) -> Result<Due, String> {
    if let Ok(at) = NaiveDateTime::parse_from_str(input.trim(), DATE_FMT) {
        return Ok(Due::At(at));
    }
    input.parse().map(Due::Rule).map_err(|_| {
        // the examples are written for the TUI, with a middle dot between them
        let rules = DeadlineRule::EXAMPLES.replace(" · ", ", ");
        format!("invalid due {input:?}: use a rule like {rules}, or YYYY-MM-DD HH:MM")
    })
}

#[cfg(test)]
mod tests {
    use chrono::NaiveDate;

    use super::*;

    fn at(d: u32, h: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(2026, 10, d)
            .unwrap()
            .and_hms_opt(h, 0, 0)
            .unwrap()
    }

    #[test]
    fn due_takes_a_rule() {
        let due = due("fri 22:00").unwrap();

        assert_eq!(due.after(at(15, 12)), at(16, 22)); // Thu 15th -> Fri 16th
    }

    #[test]
    fn due_takes_a_date() {
        let due = due("2026-10-20 09:00").unwrap();

        assert_eq!(due.after(at(15, 12)), at(20, 9)); // `now` does not matter
    }

    #[test]
    fn a_bad_due_names_both_forms_in_ascii() {
        let err = due("someday").unwrap_err();

        assert!(err.contains("fri 22:00") && err.contains("YYYY-MM-DD HH:MM"), "{err}");
        assert!(err.is_ascii(), "{err}");
    }
}
