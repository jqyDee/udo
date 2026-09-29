use std::{fmt, str::FromStr};

use serde::{Deserialize, Serialize};

/// A length of time in whole minutes, written like `1h30`, `2h` or `45m`
/// (in files, forms and the CLI).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Minutes(u32);

impl Minutes {
    pub fn new(minutes: u32) -> Self {
        Self(minutes)
    }

    pub fn get(self) -> u32 {
        self.0
    }

    /// Ways to write a duration, `·`-separated, for hints in the UI. Each
    /// one parses, and the error message (`FORMAT`) names them too (tested).
    pub const EXAMPLES: &str = "1h30 · 2h · 45m";
}

const FORMAT: &str = "write durations like 1h30, 1h30m, 2h or 45m";

impl FromStr for Minutes {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.trim().to_lowercase();
        let (hours, minutes) = match s.split_once('h') {
            // "2h", "1h30", "1h30m"
            Some((h, m)) => {
                let m = m.strip_suffix('m').unwrap_or(m);
                let minutes = if m.is_empty() { 0 } else { number(m)? };
                if minutes >= 60 {
                    return Err(format!("{s}: minutes after hours must be below 60"));
                }
                (number(h)?, minutes)
            }
            // "45m"; no unit at all ("90") is an error
            None => (0, number(s.strip_suffix('m').ok_or(FORMAT)?)?),
        };
        hours
            .checked_mul(60)
            .and_then(|h| h.checked_add(minutes))
            .map(Minutes)
            .ok_or_else(|| "duration is too long".to_string())
    }
}

/// Short form: `45m`, `2h`, `1h05`. Always parses back to the same value.
impl fmt::Display for Minutes {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match (self.0 / 60, self.0 % 60) {
            (0, m) => write!(f, "{m}m"),
            (h, 0) => write!(f, "{h}h"),
            (h, m) => write!(f, "{h}h{m:02}"),
        }
    }
}

/// Read from files (`serde(try_from = "String")`).
impl TryFrom<String> for Minutes {
    type Error = String;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        s.parse()
    }
}

/// Written to files (`serde(into = "String")`).
impl From<Minutes> for String {
    fn from(m: Minutes) -> Self {
        m.to_string()
    }
}

/// Estimate minus time so far (the details' `left` row).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Left {
    /// This much is left (exactly on the estimate: `0m`).
    Left(Minutes),
    /// The estimate is exceeded by this much.
    Over(Minutes),
}

impl Left {
    /// What is left of `estimate` after `duration`.
    pub fn of(estimate: Minutes, duration: Minutes) -> Self {
        match estimate.get().checked_sub(duration.get()) {
            Some(left) => Self::Left(Minutes::new(left)),
            None => Self::Over(Minutes::new(duration.get() - estimate.get())),
        }
    }
}

/// Digits only: no sign, no decimals, not empty.
fn number(s: &str) -> Result<u32, String> {
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
        return Err(FORMAT.into());
    }
    s.parse().map_err(|_| "duration is too long".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(s: &str) -> Result<Minutes, String> {
        s.parse()
    }

    fn m(minutes: u32) -> Minutes {
        Minutes::new(minutes)
    }

    #[test]
    fn left_of_the_estimate() {
        assert_eq!(Left::of(m(120), m(72)), Left::Left(m(48)));
        assert_eq!(Left::of(m(60), m(60)), Left::Left(m(0))); // exactly on it
        assert_eq!(Left::of(m(60), m(80)), Left::Over(m(20)));
        assert_eq!(Left::of(m(0), m(5)), Left::Over(m(5)));
    }

    #[test]
    fn parses_every_accepted_form() {
        for (input, minutes) in [
            ("45m", 45),
            ("90m", 90),
            ("0m", 0),
            ("2h", 120),
            ("1h30", 90),
            ("1h30m", 90),
            ("1h05", 65),
            ("1h5", 65),
            ("0h45", 45),
            (" 1H30 ", 90),
            ("2H", 120),
        ] {
            assert_eq!(parse(input), Ok(Minutes(minutes)), "{input:?}");
        }
    }

    #[test]
    fn rejects_bad_input() {
        for input in [
            "", "  ", "90", "m", "h", "h30", "1x", "1.5h", "1,5h", "-5m", "+5m", "1h-5", "1h75",
            "1h60", "1h30x", "1 h 30", "1h30mm", "30m1h",
        ] {
            assert!(parse(input).is_err(), "{input:?} was accepted");
        }
    }

    #[test]
    fn examples_parse_and_the_error_names_them() {
        for example in Minutes::EXAMPLES.split(" · ") {
            assert!(parse(example).is_ok(), "{example:?}");
            assert!(FORMAT.contains(example), "{example:?} not in {FORMAT:?}");
        }
    }

    #[test]
    fn errors_say_what_is_wrong() {
        assert_eq!(parse("90"), Err(FORMAT.to_string())); // no unit
        assert!(parse("1h75").unwrap_err().contains("below 60"));
        assert!(parse("99999999h").unwrap_err().contains("too long"));
    }

    #[test]
    fn overflow_is_an_error_not_a_panic() {
        assert!(parse("99999999h").is_err()); // * 60 overflows u32
        assert!(parse("99999999999m").is_err()); // doesn't fit u32 at all
        assert!(parse("71582789h").is_err()); // 71582789 * 60 > u32::MAX
        assert!(parse("71582788h59").is_err()); // only the add overflows
    }

    #[test]
    fn displays_short_form() {
        for (minutes, text) in [
            (0, "0m"),
            (45, "45m"),
            (60, "1h"),
            (65, "1h05"),
            (90, "1h30"),
            (120, "2h"),
            (1500, "25h"),
        ] {
            assert_eq!(Minutes(minutes).to_string(), text);
        }
    }

    #[test]
    fn display_parses_back() {
        for n in 0..=600 {
            let m = Minutes(n);
            assert_eq!(parse(&m.to_string()), Ok(m), "{m}");
        }
    }

    #[test]
    fn sorts_by_length() {
        let mut v = vec![Minutes(90), Minutes(5), Minutes(60)];
        v.sort();
        assert_eq!(v, [Minutes(5), Minutes(60), Minutes(90)]);
    }

    // ---------- file format ----------

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Row {
        estimate: Minutes,
    }

    #[test]
    fn toml_roundtrip_as_text() {
        let row = Row {
            estimate: Minutes(90),
        };
        let text = toml::to_string(&row).unwrap();
        assert_eq!(text.trim(), "estimate = \"1h30\"");
        assert_eq!(toml::from_str::<Row>(&text).unwrap(), row);
    }

    #[test]
    fn toml_rejects_bad_durations() {
        let err = toml::from_str::<Row>("estimate = \"90\"").unwrap_err();
        assert!(err.to_string().contains(FORMAT), "got: {err}");
        assert!(toml::from_str::<Row>("estimate = 90").is_err()); // a number, not text
    }
}
