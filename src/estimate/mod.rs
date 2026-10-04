//! Estimates: how long a task will take, learned from the recorded time.
//! Pure: knows `model` types only, no `Core`, no I/O, no async. `Core`
//! builds the `History`, the estimators read it.

mod average;
mod history;
#[cfg(test)]
mod tests;

use crate::model::{id::NodeId, node::Node, time::Minutes};

pub use average::{Average, K};
pub use history::{History, TaskRecord};

/// What udo estimates for a task, and why.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Estimate {
    pub minutes: Minutes,
    pub basis: Basis,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Basis {
    /// Only a prior: no counted tasks yet.
    Prior(Prior),
    /// Blended from counted tasks and (if any) a prior.
    Learned {
        container: NodeId,
        tasks: usize,
        prior: Option<Prior>,
    },
}

/// Where the starting value of a blend comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Prior {
    /// The `estimate` setting set on the container.
    Setting { container: NodeId, minutes: Minutes },
    /// The parent's estimate, pooled over its subtree without the child.
    Parent {
        container: NodeId,
        tasks: usize,
        minutes: Minutes,
    },
}

impl Prior {
    /// The starting value, wherever it comes from.
    pub fn minutes(&self) -> Minutes {
        match self {
            Prior::Setting { minutes, .. } | Prior::Parent { minutes, .. } => *minutes,
        }
    }
}

/// One way to estimate. Methods, not associated consts: keeps `dyn
/// Estimator` possible.
pub trait Estimator {
    /// Recorded with every estimate row: `average`, later `median`, ...
    fn method(&self) -> &'static str;
    /// Of the method; a changed formula gets a new version.
    fn version(&self) -> u32;
    /// `None`: no prior and no counted tasks.
    fn estimate(&self, task: NodeId, history: &History) -> Option<Estimate>;
}

/// The estimate shown for `node`: a task's (its container's, without
/// itself), or a container's (pooled over its subtree, "a typical task
/// anywhere in it").
pub fn of_node(node: &Node, history: &History) -> Option<Estimate> {
    match node.as_task() {
        Some(_) => Average.estimate(node.id(), history),
        None => Average.of_container(node.id(), history),
    }
}
