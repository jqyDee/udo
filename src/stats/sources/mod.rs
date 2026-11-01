//! Sources: where the intervals come from. One file per source. Pure: no
//! `Core`, no I/O, no async.

mod sessions;

pub use sessions::SessionsSource;

use crate::{model::time::Time, stats::Interval};

pub trait Source {
    /// Every interval intersecting `[from, to)`. A running one ends at `now`.
    fn intervals(&self, from: Time, to: Time, now: Time) -> Vec<Interval>;
}
