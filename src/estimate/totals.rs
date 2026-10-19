//! Estimate vs. actual (#9): every task's `Shown` once, and the sums over
//! the tasks below a node.

use std::collections::HashMap;

use crate::{
    estimate::{History, Shown, of_node},
    model::{
        id::NodeId,
        time::{Left, Minutes},
        tree::Tree,
    },
};

/// What `of_node` gives for every task in the tree, computed once (TUI:
/// per `reload`). A container's totals are lookups, not new estimates.
#[derive(Debug, Clone, Default)]
pub struct Estimates(HashMap<NodeId, Shown>);

impl Estimates {
    /// Tasks only: containers get no entry (their numbers are `Totals`).
    pub fn build(tree: &Tree, history: &History) -> Self {
        Self(
            tree.rows()
                .into_iter()
                .filter(|r| r.node.as_task().is_some())
                .map(|r| (r.node.id(), of_node(r.node, history)))
                .collect(),
        )
    }

    /// `Shown::default()` for an unknown id (both `None`), containers included.
    pub fn get(&self, id: NodeId) -> Shown {
        self.0.get(&id).copied().unwrap_or_default()
    }
}

/// Estimate vs. actual over every task at or below a node.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Totals {
    /// Tasks below (deleted ones are not in the tree: not counted).
    pub tasks: usize,
    /// Tasks with an estimate (yours or learned).
    pub estimated_tasks: usize,
    /// Σ estimate of those.
    pub estimated: Minutes,
    /// Σ tracked time of all `tasks` (a running session up to `now`).
    pub actual: Minutes,
    /// Open tasks with an estimate: Σ max(estimate − actual, 0).
    pub left: Minutes,
    /// Open tasks already over their estimate.
    pub over: usize,
    /// Done tasks with an estimate: Σ estimate and Σ actual.
    pub done_estimated: Minutes,
    pub done_actual: Minutes,
}

/// The totals of the node at `path`; a task: itself alone. Tasks from
/// `tree.task_ids_below(path)`, estimates from `estimates`, actual time
/// from `history`.
pub fn totals(tree: &Tree, path: &[usize], estimates: &Estimates, history: &History) -> Totals {
    let mut t = Totals::default();
    for id in tree.task_ids_below(path) {
        let record = history.task(id);
        let actual = record.map_or(Minutes::default(), |r| r.actual);
        let done = record.is_some_and(|r| r.done_at.is_some());

        t.tasks += 1;
        t.actual += actual;
        let Some(estimate) = estimates.get(id).minutes() else {
            continue; // counted in `tasks`, not in the estimate sums
        };
        t.estimated_tasks += 1;
        t.estimated += estimate;
        if done {
            t.done_estimated += estimate;
            t.done_actual += actual;
        } else {
            match Left::of(estimate, actual) {
                Left::Left(left) => t.left += left,
                Left::Over(_) => t.over += 1, // 0 in `left`, never negative
            }
        }
    }

    t
}
