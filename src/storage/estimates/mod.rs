//! Where estimate rows are stored: `EstimateStore` backends (memory,
//! SQLite) and the contract they share.

use crate::{
    model::{
        estimate_store::{EstimateError, EstimateStore, NewEstimate, Recorded},
        id::NodeId,
    },
    storage::{
        dispatch::dispatch,
        estimates::{memory::MemoryEstimates, sqlite::SqliteEstimates},
    },
};

#[cfg(test)]
mod contract;
pub mod memory;
pub mod sqlite;

pub enum Estimates {
    Memory(MemoryEstimates),
    Sqlite(SqliteEstimates),
}

impl EstimateStore for Estimates {
    async fn record(&self, row: NewEstimate) -> Result<Recorded, EstimateError> {
        dispatch!(self, e => e.record(row))
    }

    async fn last_of(&self, task: NodeId) -> Result<Option<Recorded>, EstimateError> {
        dispatch!(self, e => e.last_of(task))
    }

    async fn of_tasks(&self, tasks: &[NodeId]) -> Result<Vec<Recorded>, EstimateError> {
        dispatch!(self, e => e.of_tasks(tasks))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{model::time::Clock, storage::sqlite};

    mod memory {
        use super::*;

        contract::estimate_store_contract!(|clock: Clock| Estimates::Memory(MemoryEstimates::new(
            clock
        )));
    }

    mod sqlite_in_memory {
        use super::*;

        contract::estimate_store_contract!(|clock: Clock| {
            Estimates::Sqlite(SqliteEstimates::new(
                sqlite::open_in_memory().unwrap(),
                clock,
            ))
        });
    }
}
