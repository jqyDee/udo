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
    async fn record(&self, row: NewEstimate) -> Result<Recorded, EstimateError> {
        let recorded = Recorded {
            id: Uuid::now_v7(),
            estimate: row,
            at: to_ms((self.clock)()),
        };
        self.rows.lock().unwrap().push(recorded.clone());
        Ok(recorded)
    }

    async fn last_of(&self, task: NodeId) -> Result<Option<Recorded>, EstimateError> {
        let rows = self.rows.lock().unwrap();
        Ok(rows
            .iter()
            .filter(|r| r.estimate.task == task)
            .max_by_key(|r| (r.at, r.id))
            .cloned())
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

#[cfg(test)]
mod tests {
    use super::*;

    super::super::contract::estimate_store_contract!(MemoryEstimates::new);
}
