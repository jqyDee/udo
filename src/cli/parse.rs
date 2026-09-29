//! clap value parsers: text from the command line -> typed values.

use chrono::{NaiveDateTime, TimeDelta};

use crate::{
    DATE_FMT, Res,
    model::{
        container::ContainerKind,
        task::TaskStatus,
        time::{DeadlineRule, Minutes, Time, local_to_fixed},
    },
};

/// `udo mark STATUS`: the statuses as they are typed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum StatusArg {
    Todo,
    InProgress,
    Stale,
    Done,
}

impl From<StatusArg> for TaskStatus {
    fn from(arg: StatusArg) -> Self {
        match arg {
            StatusArg::Todo => Self::Pending,
            StatusArg::InProgress => Self::InProgress,
            StatusArg::Stale => Self::Stale,
            StatusArg::Done => Self::Finished,
        }
    }
}

/// `udo edit --kind`: the kinds a container can be changed to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum KindArg {
    Workspace,
    Project,
}

impl From<KindArg> for ContainerKind {
    fn from(arg: KindArg) -> Self {
        match arg {
            KindArg::Workspace => Self::Workspace,
            KindArg::Project => Self::Project,
        }
    }
}

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

/// A session time (`list --from / --to`, `add`, `edit`, `split`, `cut`),
/// all local. Sessions record what happened: `resolve` refuses the future.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionTime {
    /// `2026-10-15 14:00`
    At(NaiveDateTime),
    /// `now`
    Now,
    /// `-45m`: this long before now.
    Ago(Minutes),
    /// `+1h30`: this long after the first time of a pair (only as the
    /// second one: `add`'s END, `cut`'s TO, `list`'s `--to`).
    After(Minutes),
}

impl SessionTime {
    /// The point in time this means at `now`; `first` is the pair's first
    /// time for `+D`. After `now` or skipped by a DST switch: an error.
    pub fn resolve(self, now: Time, first: Option<Time>) -> Res<Time> {
        let t = match self {
            Self::At(local) => local_to_fixed(local).ok_or_else(|| {
                format!("{} does not exist here (skipped by a DST switch)", local.format(DATE_FMT))
            })?,
            Self::Now => now,
            Self::Ago(d) => now - TimeDelta::minutes(d.get().into()),
            Self::After(d) => {
                let first = first.ok_or("+D is only allowed as the second time of a pair")?;
                first + TimeDelta::minutes(d.get().into())
            }
        };
        if t > now {
            return Err("that time is in the future: sessions record what happened".into());
        }
        Ok(t)
    }
}

/// Parse a session time: `YYYY-MM-DD HH:MM`, `now`, `-D` or `+D` (`D` like
/// `1h30` / `45m`). Nothing else: see the spec for why no `14:00`.
pub fn session_time(input: &str) -> Result<SessionTime, String> {
    let input = input.trim();
    let bad = || {
        format!(
            "invalid time {input:?}: use YYYY-MM-DD HH:MM, now, -45m (before now) \
             or +1h30 (after the first time)"
        )
    };
    if input == "now" {
        return Ok(SessionTime::Now);
    }
    if let Some(d) = input.strip_prefix('-') {
        return d.parse().map(SessionTime::Ago).map_err(|_| bad());
    }
    if let Some(d) = input.strip_prefix('+') {
        return d.parse().map(SessionTime::After).map_err(|_| bad());
    }
    NaiveDateTime::parse_from_str(input, DATE_FMT)
        .map(SessionTime::At)
        .map_err(|_| bad())
}

#[cfg(test)]
mod tests {
    use chrono::NaiveDate;

    use super::*;
    use crate::test_util::{self, dt};

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

    // ---------- session times ----------

    /// "Now" for the session time tests: 2026-10-15 20:00 +02:00.
    fn now() -> Time {
        test_util::at(20, 0)
    }

    #[test]
    fn session_time_forms_parse() {
        assert_eq!(
            session_time("2026-10-15 14:00").unwrap(),
            SessionTime::At(dt(2026, 10, 15, 14, 0))
        );
        assert_eq!(session_time("now").unwrap(), SessionTime::Now);
        assert_eq!(session_time("-1h30").unwrap(), SessionTime::Ago(Minutes::new(90)));
        assert_eq!(session_time("+45m").unwrap(), SessionTime::After(Minutes::new(45)));
    }

    #[test]
    fn other_session_time_forms_are_refused_in_ascii() {
        for bad in ["14:00", "yesterday", "fri 22:00", "-90", "+", ""] {
            let err = session_time(bad).unwrap_err();
            assert!(err.contains("YYYY-MM-DD HH:MM") && err.contains("-45m"), "{bad}: {err}");
            assert!(err.is_ascii(), "{err}");
        }
    }

    #[test]
    fn a_date_is_local_time() {
        let t = SessionTime::At(dt(2026, 10, 15, 14, 0))
            .resolve(now(), None)
            .unwrap();

        assert_eq!(t, local_to_fixed(dt(2026, 10, 15, 14, 0)).unwrap());
    }

    #[test]
    fn now_and_ago_count_from_now() {
        assert_eq!(SessionTime::Now.resolve(now(), None).unwrap(), now());
        let ago = SessionTime::Ago(Minutes::new(45)).resolve(now(), None);
        assert_eq!(ago.unwrap(), test_util::at(19, 15));
    }

    #[test]
    fn after_counts_from_the_first_time() {
        let first = test_util::at(14, 0);

        let t = SessionTime::After(Minutes::new(90)).resolve(now(), Some(first));

        assert_eq!(t.unwrap(), test_util::at(15, 30));
    }

    #[test]
    fn after_without_a_first_time_is_refused() {
        let err = SessionTime::After(Minutes::new(60))
            .resolve(now(), None)
            .unwrap_err()
            .to_string();

        assert!(err.contains("second time"), "{err}");
        assert!(!err.contains("future"), "{err}");
    }

    #[test]
    fn a_future_time_is_refused() {
        let future =
            SessionTime::After(Minutes::new(60)).resolve(now(), Some(test_util::at(19, 30)));
        assert!(future.unwrap_err().to_string().contains("future"));
        let tomorrow = SessionTime::At(dt(2026, 10, 16, 9, 0)).resolve(now(), None);
        assert!(tomorrow.is_err());
    }

    #[test]
    fn a_time_skipped_by_dst_is_refused() {
        // only testable where the local zone has a gap then (e.g. Europe)
        let gap = dt(2026, 3, 29, 2, 30);
        if local_to_fixed(gap).is_none() {
            let err = SessionTime::At(gap).resolve(now(), None).unwrap_err();
            assert!(err.to_string().contains("does not exist"), "{err}");
        }
    }
}
