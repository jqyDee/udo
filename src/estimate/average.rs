//! `Average`: a container's tasks blended with a prior. The prior counts
//! like `K` tasks of its length, so the estimate moves from the prior to
//! the recorded time as tasks are done.

use super::{Basis, Estimate, Estimator, History, Prior, TaskRecord};
use crate::model::{id::NodeId, time::Minutes};

/// How many tasks the prior counts as.
pub const K: f64 = 3.0;

/// The weighted average of a container's tasks, blended with its prior.
/// Done tasks count fully; open ones only above the estimate from the done
/// ones, at half weight (their time so far is an "at least").
pub struct Average;

/// Which tasks a blend learns from.
enum Scope {
    /// A task's estimate: its container's direct tasks, without its one.
    Task(NodeId),
    /// A container's own estimate: every task in its subtree.
    Subtree,
}

impl Estimator for Average {
    fn method(&self) -> &'static str {
        "average"
    }

    fn version(&self) -> u32 {
        1
    }

    /// A task's estimate is its container's, without the task itself.
    fn estimate(&self, task: NodeId, history: &History) -> Option<Estimate> {
        let rec = history.task(task)?;
        self.learn(rec.container, Scope::Task(task), history)
    }
}

impl Average {
    /// "A typical task anywhere in it": every task in the container's
    /// subtree counts, not only its direct ones; the prior as for a task.
    /// So a workspace without tasks of its own still has an estimate, the
    /// same one a new, empty child gets as its prior (`udo estimate` on a
    /// container, #9).
    pub fn of_container(&self, container: NodeId, history: &History) -> Option<Estimate> {
        self.learn(container, Scope::Subtree, history)
    }

    /// The estimate of `container` from its direct tasks (but `without`)
    /// and its prior (`prior`).
    fn learn(&self, container: NodeId, scope: Scope, history: &History) -> Option<Estimate> {
        let prior = self.prior(container, history);
        let tasks = history.tasks().iter().filter(|t| match scope {
            Scope::Task(id) => t.container == container && t.id != id,
            Scope::Subtree => history.is_below(t.container, container),
        });
        let (minutes, tasks) = blend(prior.map(|p| p.minutes()), tasks)?;
        let basis = if tasks == 0 {
            Basis::Prior(prior?) // nothing counted: blend only answered because of the prior
        } else {
            Basis::Learned {
                container,
                tasks,
                prior,
            }
        };
        Some(Estimate { minutes, basis })
    }

    /// Where `container`'s blend starts: its own `estimate` setting, else
    /// the parent's estimate pooled over the parent's subtree without
    /// `container`'s subtree, else whatever the parent starts from.
    fn prior(&self, container: NodeId, history: &History) -> Option<Prior> {
        // 1. an own setting cuts the chain ("this course is different")
        if let Some(minutes) = history.own_estimate(container) {
            return Some(Prior::Setting { container, minutes });
        }

        // 2. the root without a setting: nothing to start from
        let parent = history.parent(container)?;

        // 3. what the parent knows, without this container's tasks
        let above = self.prior(parent, history);
        let pool = history.tasks().iter().filter(|t| {
            history.is_below(t.container, parent) && !history.is_below(t.container, container)
        });
        match blend(above.map(|p| p.minutes()), pool) {
            Some((minutes, tasks)) if tasks > 0 => Some(Prior::Parent {
                container: parent,
                tasks,
                minutes,
            }),
            _ => above, // the parent's subtree adds nothing: pass its prior on
        }
    }
}

/// `(K·prior + Σ w·actual) / (K + Σ w)`, rounded, and how many tasks
/// counted. Tasks without tracked time are skipped; done tasks have
/// `w = 1`; open ones `w = ½`, only if above the estimate from the done
/// ones. No prior: `K = 0`. `None`: no prior and nothing done (then open
/// tasks have nothing to be compared with either).
fn blend<'a>(
    prior: Option<Minutes>,
    tasks: impl Iterator<Item = &'a TaskRecord>,
) -> Option<(Minutes, usize)> {
    let (done, open): (Vec<&TaskRecord>, Vec<&TaskRecord>) = tasks
        .filter(|t| t.actual.get() > 0)
        .partition(|t| t.done_at.is_some()); // open and deleted: `done_at` None

    if prior.is_none() && done.is_empty() {
        return None; // no weight: the base below would be 0 / 0
    }

    let (k, p) = match prior {
        Some(m) => (K, f64::from(m.get())),
        None => (0.0, 0.0), // no prior: the plain average
    };
    let mut weight = k;
    let mut sum = k * p;

    for t in &done {
        weight += 1.0;
        sum += f64::from(t.actual.get());
    }
    let base = sum / weight; // the estimate from done tasks only

    // every open task against the same base, so their order doesn't matter
    let mut counted = done.len();
    for t in &open {
        let actual = f64::from(t.actual.get());
        if actual > base {
            weight += 0.5;
            sum += 0.5 * actual;
            counted += 1;
        }
    }

    let minutes = (sum / weight).round() as u32; // weight > 0 (checked above), never negative
    Some((Minutes::new(minutes), counted))
}
