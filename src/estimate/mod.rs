//! Estimates: how long a task will take, learned from the recorded time,
//! or set by you on the task (#6), which wins. Pure: knows `model` types
//! only, no `Core`, no I/O, no async. `Core` builds the `History`, the
//! estimators read it.

mod average;
mod frozen;
mod history;
mod manual;
pub mod store;
#[cfg(test)]
mod tests;
pub mod text;
mod totals;

use crate::model::{
    id::NodeId,
    node::Node,
    task::Task,
    time::{Minutes, Time},
};

pub use average::{Average, K};
pub use frozen::{Frozen, from_rows, replay};
pub use history::{History, TaskRecord};
pub use manual::Manual;
pub use totals::{Estimates, Totals, totals};

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
    /// The task's own estimate (`Task::estimate`), set by you; only
    /// `Manual` gives it.
    Own,
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

/// What is shown for a node: your own estimate and udo's learned one, side
/// by side, so they can be compared. Both `None`: nothing set, nothing to
/// learn from.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Shown {
    /// The task's own estimate; `None` on containers or when not set.
    pub own: Option<Minutes>,
    /// What udo learned (a done task's frozen, see `of_node`).
    pub learned: Option<Estimate>,
}

impl Shown {
    /// What udo goes by (`left`): yours, else the learned one. The same
    /// order as `pick`.
    pub fn minutes(&self) -> Option<Minutes> {
        self.own.or(self.learned.map(|e| e.minutes))
    }
}

/// What is shown for `node`. A task: its own estimate, read from the node
/// (current, never frozen), and the learned one: an open task's (its
/// container's, without itself, from today's data), a done task's (frozen:
/// as udo would have given it at `TaskRecord::frozen_at`, so it never
/// changes afterwards and sees no later task): from `frozen` (its recorded
/// row, #60), replayed only when `frozen` has no entry for it. A
/// container: only the learned one, pooled over its subtree ("a typical
/// task anywhere in it").
pub fn of_node(node: &Node, history: &History, frozen: &Frozen) -> Shown {
    match node.as_task() {
        Some(t) => Shown {
            own: t.estimate,
            learned: learned_of_task(node.id(), t, history, frozen),
        },
        None => Shown {
            own: None,
            learned: Average.of_container(node.id(), history),
        },
    }
}

/// The learned estimate of `task` (with id `id`), frozen if it is done.
fn learned_of_task(
    id: NodeId,
    task: &Task,
    history: &History,
    frozen: &Frozen,
) -> Option<Estimate> {
    match task.done_at {
        None => Average.estimate(id, history),
        Some(_) => frozen.get(id).unwrap_or_else(|| replay(id, history)),
    }
}

/// The estimators, yours first: the one place the order lives (no chain:
/// YAGNI; a third estimator, e.g. AI #13, is one more entry).
const ORDER: [&dyn Estimator; 2] = [&Manual, &Average];

/// The estimate udo goes by for `task`: yours, else the learned one, with
/// the estimator that gave it (its `method` / `version` go into the
/// history row). Not frozen: callers that want a done task's estimate pass
/// `history.as_of(..)`.
pub fn pick(task: NodeId, history: &History) -> Option<(Estimate, &'static dyn Estimator)> {
    ORDER
        .into_iter()
        .find_map(|e| Some((e.estimate(task, history)?, e)))
}

/// Every estimator's answer for `task`, in `pick`'s order: what is recorded
/// at a task's `created` / `started` moments, yours and the learned one
/// side by side (#60).
pub fn each(task: NodeId, history: &History) -> Vec<(Estimate, &'static dyn Estimator)> {
    ORDER
        .into_iter()
        .filter_map(|e| Some((e.estimate(task, history)?, e)))
        .collect()
}
