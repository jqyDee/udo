use std::sync::Mutex;

use uuid::Uuid;

use crate::{
    model::{
        estimate_store::{EstimateError, EstimateStore, NewEstimate, Recorded},
        id::NodeId,
        time::Clock,
    },
    storage::time::to_ms,
};

/// Every row, in the order written.
pub struct MemoryEstimates {
    rows: Mutex<Vec<Recorded>>,
    clock: Clock,
}

impl MemoryEstimates {
    pub fn new(clock: Clock) -> Self {
        Self {
            rows: Mutex::new(Vec::new()),
            clock,
        }
    }
}

impl EstimateStore for MemoryEstimates {
    async fn record(&self, row: NewEstimate) -> Result<Option<Recorded>, EstimateError> {
        let mut rows = self.rows.lock().unwrap(); // held until the push: one step
        if latest(&rows, row.task).is_some_and(|last| row.repeats(&last.estimate)) {
            return Ok(None);
        }
        let recorded = Recorded {
            id: Uuid::now_v7(),
            estimate: row,
            at: to_ms((self.clock)()),
        };
        rows.push(recorded.clone());
        Ok(Some(recorded))
    }

    async fn last_of(&self, task: NodeId) -> Result<Option<Recorded>, EstimateError> {
        Ok(latest(&self.rows.lock().unwrap(), task).cloned())
    }

    async fn of_tasks(&self, tasks: &[NodeId]) -> Result<Vec<Recorded>, EstimateError> {
        let mut found: Vec<Recorded> = self
            .rows
            .lock()
            .unwrap()
            .iter()
            .filter(|r| tasks.contains(&r.estimate.task))
            .cloned()
            .collect();
        found.sort_by_key(|r| (r.at, r.id));
        Ok(found)
    }
}

/// The latest row of `task` in `rows` (by time, then id).
fn latest(rows: &[Recorded], task: NodeId) -> Option<&Recorded> {
    rows.iter()
        .filter(|r| r.estimate.task == task)
        .max_by_key(|r| (r.at, r.id))
}
