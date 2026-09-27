use std::{fmt, str::FromStr};

use chrono::{Datelike, Days, NaiveDateTime, NaiveTime, Weekday};
use serde::{Deserialize, Serialize};

/// Rule for the due date of new tasks, written like `fri 22:00` (next
/// Friday) or `+7d 23:59` (in 7 days). Local time, like the form.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub enum DeadlineRule {
    /// Next `day` at `time`, strictly after now.
    Weekday { day: Weekday, time: NaiveTime },
    /// `days` from today at `time`.
    InDays { days: u32, time: NaiveTime },
}

const FORMAT: &str = "write deadlines like fri 22:00 or +7d 23:59";

impl DeadlineRule {
    /// The due date this rule gives for a task created at `now` (local,
    /// naive: the form turns it into UTC on submit).
    pub fn next_after(self, now: NaiveDateTime) -> NaiveDateTime {
        match self {
            Self::InDays { days, time } => now
                .date()
                .checked_add_days(Days::new(days.into()))
                .map_or(now, |date| date.and_time(time)), // absurdly far: fall back to now
            Self::Weekday { day, time } => {
                let today = now.weekday().num_days_from_monday();
                let ahead = (day.num_days_from_monday() + 7 - today) % 7; // 0 = today
                let at = (now.date() + Days::new(ahead.into())).and_time(time);
                if at > now { at } else { at + Days::new(7) }
            }
        }
    }
}

impl FromStr for DeadlineRule {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.trim().to_lowercase();
        let (when, time) = s.split_once(char::is_whitespace).ok_or(FORMAT)?;
        let time = NaiveTime::parse_from_str(time.trim(), "%H:%M").map_err(|_| FORMAT)?;
        match when.strip_prefix('+') {
            // "+7d"
            Some(days) => {
                let days = days.strip_suffix('d').ok_or(FORMAT)?;
                if days.is_empty() || !days.bytes().all(|b| b.is_ascii_digit()) {
                    return Err(FORMAT.into());
                }
                let days = days.parse().map_err(|_| "deadline is too far away")?;
                Ok(Self::InDays { days, time })
            }
            // "fri", "friday"
            None => {
                let day = when
                    .parse()
                    .map_err(|_| format!("{when:?} is not a weekday, {FORMAT}"))?;
                Ok(Self::Weekday { day, time })
            }
        }
    }
}

/// Short form: `fri 22:00`, `+7d 23:59`. Always parses back to the same rule.
impl fmt::Display for DeadlineRule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Weekday { day, time } => {
                write!(f, "{} {}", weekday_name(*day), time.format("%H:%M"))
            }
            Self::InDays { days, time } => write!(f, "+{days}d {}", time.format("%H:%M")),
        }
    }
}

/// Lowercase short name (chrono's `Display` gives `Fri`).
fn weekday_name(day: Weekday) -> &'static str {
    match day {
        Weekday::Mon => "mon",
        Weekday::Tue => "tue",
        Weekday::Wed => "wed",
        Weekday::Thu => "thu",
        Weekday::Fri => "fri",
        Weekday::Sat => "sat",
        Weekday::Sun => "sun",
    }
}

/// Read from files (`serde(try_from = "String")`).
impl TryFrom<String> for DeadlineRule {
    type Error = String;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        s.parse()
    }
}

/// Written to files (`serde(into = "String")`).
impl From<DeadlineRule> for String {
    fn from(rule: DeadlineRule) -> Self {
        rule.to_string()
    }
}

#[cfg(test)]
mod tests {
    use chrono::NaiveDate;

    use super::*;

    fn parse(s: &str) -> Result<DeadlineRule, String> {
        s.parse()
    }

    fn hm(h: u32, m: u32) -> NaiveTime {
        NaiveTime::from_hms_opt(h, m, 0).unwrap()
    }

    /// `month`/`day` 2026 at `h`:`m`. 2026-09-27 is a Sunday.
    fn at(month: u32, day: u32, h: u32, m: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(2026, month, day)
            .unwrap()
            .and_time(hm(h, m))
    }

    // ---------- parsing ----------

    #[test]
    fn parses_every_accepted_form() {
        let fri_22 = DeadlineRule::Weekday {
            day: Weekday::Fri,
            time: hm(22, 0),
        };
        for (input, rule) in [
            ("fri 22:00", fri_22),
            ("Friday 22:00", fri_22),
            (" FRI   22:00 ", fri_22),
            (
                "mon 08:30",
                DeadlineRule::Weekday {
                    day: Weekday::Mon,
                    time: hm(8, 30),
                },
            ),
            (
                "+7d 23:59",
                DeadlineRule::InDays {
                    days: 7,
                    time: hm(23, 59),
                },
            ),
            (
                "+0d 18:00",
                DeadlineRule::InDays {
                    days: 0,
                    time: hm(18, 0),
                },
            ),
        ] {
            assert_eq!(parse(input), Ok(rule), "{input:?}");
        }
    }

    #[test]
    fn rejects_bad_input() {
        for input in [
            "",
            "fri",
            "22:00",
            "7d 23:59",
            "+7 23:59",
            "+d 23:59",
            "+-1d 12:00",
            "++1d 12:00",
            "+1.5d 12:00",
            "fri 25:00",
            "fri 22:60",
            "fri 9pm",
            "fri 22:00 extra",
            "fr 22:00",
            "tomorrow 12:00",
            "+99999999999d 12:00",
        ] {
            assert!(parse(input).is_err(), "{input:?} was accepted");
        }
    }

    #[test]
    fn errors_say_what_is_wrong() {
        assert_eq!(parse("fri"), Err(FORMAT.to_string()));
        assert!(
            parse("tomorrow 12:00")
                .unwrap_err()
                .contains("not a weekday")
        );
        assert!(
            parse("+99999999999d 12:00")
                .unwrap_err()
                .contains("too far")
        );
    }

    #[test]
    fn displays_short_form_and_parses_back() {
        for text in [
            "fri 22:00",
            "mon 08:30",
            "sun 00:00",
            "+7d 23:59",
            "+0d 18:00",
        ] {
            let rule = parse(text).unwrap();
            assert_eq!(rule.to_string(), text);
            assert_eq!(parse(&rule.to_string()), Ok(rule));
        }
        assert_eq!(parse("Friday 9:05").unwrap().to_string(), "fri 09:05");
    }

    // ---------- next_after (2026-09-27 is a Sunday) ----------

    #[test]
    fn next_after_examples() {
        for (now, rule, expected) in [
            // next Friday
            (at(9, 27, 14, 0), "fri 22:00", at(10, 2, 22, 0)),
            // it's Friday and 22:00 is still ahead: today
            (at(10, 2, 20, 0), "fri 22:00", at(10, 2, 22, 0)),
            // it's Friday and 22:00 has passed: next week
            (at(10, 2, 23, 0), "fri 22:00", at(10, 9, 22, 0)),
            // exactly now: strictly after, so next week
            (at(10, 2, 22, 0), "fri 22:00", at(10, 9, 22, 0)),
            // Sunday -> Monday is one day
            (at(9, 27, 14, 0), "mon 08:00", at(9, 28, 8, 0)),
            // in 7 days
            (at(9, 27, 14, 0), "+7d 23:59", at(10, 4, 23, 59)),
            // across a month end
            (at(9, 30, 9, 0), "+1d 12:00", at(10, 1, 12, 0)),
            // today, even though the time is earlier than now
            (at(9, 27, 9, 0), "+0d 18:00", at(9, 27, 18, 0)),
        ] {
            let got = parse(rule).unwrap().next_after(now);
            assert_eq!(got, expected, "{rule} from {now}");
        }
    }

    #[test]
    fn weekday_rule_is_always_within_a_week() {
        let rule = parse("wed 12:00").unwrap();
        for day in 27..=30 {
            for h in [0, 11, 12, 13, 23] {
                let now = at(9, day, h, 0);
                let due = rule.next_after(now);
                assert!(due > now, "{due} not after {now}");
                assert!(
                    due - now <= chrono::TimeDelta::days(7),
                    "{due} too far from {now}"
                );
                assert_eq!(due.weekday(), Weekday::Wed);
            }
        }
    }

    #[test]
    fn absurd_in_days_falls_back_to_now() {
        let now = at(9, 27, 14, 0);
        let rule = DeadlineRule::InDays {
            days: u32::MAX,
            time: hm(12, 0),
        };
        assert_eq!(rule.next_after(now), now);
    }

    // ---------- file format ----------

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Row {
        default_deadline: DeadlineRule,
    }

    #[test]
    fn toml_roundtrip_as_text() {
        let row = Row {
            default_deadline: parse("fri 22:00").unwrap(),
        };
        let text = toml::to_string(&row).unwrap();
        assert_eq!(text.trim(), "default_deadline = \"fri 22:00\"");
        assert_eq!(toml::from_str::<Row>(&text).unwrap(), row);
    }

    #[test]
    fn toml_rejects_bad_rules() {
        let err = toml::from_str::<Row>("default_deadline = \"fri\"").unwrap_err();
        assert!(err.to_string().contains(FORMAT), "got: {err}");
    }
}
