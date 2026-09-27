//! Time values used by settings and tasks. Both are written as text in
//! files, forms and the CLI.
//!
//! - `duration`: `Minutes`, e.g. `1h30`, `45m`
//! - `deadline`: `DeadlineRule`, e.g. `fri 22:00`, `+7d 23:59`

mod deadline;
mod duration;

pub use deadline::DeadlineRule;
pub use duration::Minutes;
