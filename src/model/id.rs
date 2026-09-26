use std::fmt;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Stable ID of a node (task or container). Survives renames and moves.
/// v7: time-ordered, so it also sorts well as a DB key later.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct NodeId(Uuid);

impl NodeId {
    /// Fresh random id. No `Default` on purpose: a random default surprises.
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        Self(Uuid::now_v7())
    }
}

impl fmt::Display for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}
