//! When the event loop wakes up without a key: to redraw the running
//! timer, and to notice a timer started or stopped elsewhere.

use std::time::Duration;

use chrono::TimeDelta;

use crate::model::{sessions::Session, time::Time};

/// At most this long between two reloads.
const IDLE: Duration = Duration::from_secs(60);

/// How long from `now` until the next tick: when the shown minutes of the
/// running session change (start + shown minutes + 1), else in `IDLE`.
/// Never longer than `IDLE`, also with a clock behind the start.
pub fn next_tick(now: Time, running: Option<&Session>) -> Duration {
    match running {
        None => IDLE,
        Some(s) => {
            let shown = s.duration(now).get() as i64; // whole minutes, rounded down
            let next = s.start + TimeDelta::minutes(shown + 1);
            (next - now)
                .to_std()
                .unwrap_or(Duration::from_secs(1)) // never 0 or negative
                .min(IDLE)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::{core, parse_time};

    /// A session started at 14:00:40, through `Core` like the TUI does.
    async fn running_since_14_00_40() -> Session {
        let (_tmp, mut core) = core().await;
        core.start(&[0], parse_time("2026-10-15T14:00:40+02:00"))
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn running_wakes_when_the_shown_minutes_change() {
        let s = running_since_14_00_40().await;

        // shows 1h12 until 15:13:40
        let now = parse_time("2026-10-15T15:12:50+02:00");

        assert_eq!(next_tick(now, Some(&s)), Duration::from_secs(50));
    }

    #[tokio::test]
    async fn exactly_on_the_boundary_waits_a_full_minute() {
        let s = running_since_14_00_40().await;

        let now = parse_time("2026-10-15T15:13:40+02:00"); // 1h13 just began

        assert_eq!(next_tick(now, Some(&s)), Duration::from_secs(60));
    }

    /// Clock behind the start: shown 0m, next change far away; still a
    /// minute at most, to notice changes from elsewhere.
    #[tokio::test]
    async fn a_clock_behind_the_start_still_ticks_every_minute() {
        let s = running_since_14_00_40().await;

        let now = parse_time("2026-10-15T13:00:00+02:00");

        assert_eq!(next_tick(now, Some(&s)), Duration::from_secs(60));
    }

    #[test]
    fn idle_wakes_every_minute() {
        let now = parse_time("2026-10-15T15:12:50+02:00");

        assert_eq!(next_tick(now, None), Duration::from_secs(60));
    }
}
