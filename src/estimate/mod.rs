//! Estimates: how long a task will take, learned from the recorded time.
//! Pure: knows `model` types only, no `Core`, no I/O, no async. `Core`
//! builds the `History`, the estimators read it.

mod average;
mod history;
#[cfg(test)]
mod tests;

use crate::model::{
    id::NodeId,
    node::Node,
    time::{Minutes, Time},
};

pub use average::{Average, K};
pub use history::{History, TaskRecord};

/// What udo estimates for a task, and why.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Estimate {
    pub minutes: Minutes,
    pub basis: Basis,
    /// `Some`: a done task's estimate, as udo would have given it then
    /// (`TaskRecord::frozen_at`); `None`: from today's data.
    pub as_of: Option<Time>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Basis {
    /// Only a prior: no counted tasks yet.
    Prior(Prior),
    /// Blended from counted tasks and (if any) a prior.
    Learned {
        container: NodeId,
        /// Done tasks with tracked time, at full weight.
        done_tasks: usize,
        /// Open (or deleted) tasks whose time so far is over the estimate
        /// from the done ones, at half weight: an "at least". Open tasks
        /// under it are not counted.
        open_tasks: usize,
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

/// The estimate shown for `node`: an open task's (its container's, without
/// itself, from today's data), a done task's (frozen: as udo would have
/// given it at `TaskRecord::frozen_at`, so it never changes afterwards and
/// sees no later task), or a container's (pooled over its subtree, "a
/// typical task anywhere in it").
pub fn of_node(node: &Node, history: &History) -> Option<Estimate> {
    match node.as_task() {
        Some(t) if t.done_at.is_some() => {
            let at = history.task(node.id())?.frozen_at();
            let then = Average.estimate(node.id(), &history.as_of(at))?;
            Some(Estimate {
                as_of: Some(at),
                ..then
            })
        }
        Some(_) => Average.estimate(node.id(), history),
        None => Average.of_container(node.id(), history),
    }
}
