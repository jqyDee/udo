//! `Manual`: the task's own estimate (#6), set by you. Wins over the
//! learned one (`pick`); never learned from (`Average` ignores it).

use super::{Basis, Estimate, Estimator, History};
use crate::model::id::NodeId;

/// Gives the task's own estimate as is; `None` when it has none.
pub struct Manual;

impl Estimator for Manual {
    fn method(&self) -> &'static str {
        "manual"
    }

    fn version(&self) -> u32 {
        1
    }

    fn estimate(&self, task: NodeId, history: &History) -> Option<Estimate> {
        let minutes = history.task(task)?.own_estimate?;
        Some(Estimate {
            minutes,
            basis: Basis::Own,
            as_of: None, // yours: the current value, never frozen
        })
    }
}
