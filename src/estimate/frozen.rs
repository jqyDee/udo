//! A done task's learned estimate, frozen at its first session (#60): read
//! from its recorded row (`from_rows`), else replayed (`replay`). `Core`
//! fills `Frozen` once per build, `of_node` looks it up.

use std::collections::HashMap;

use super::{
    Average, Basis, Estimate, Estimator, History, Prior, TaskRecord,
    store::{NewEstimate, PriorSource, Reason, Recorded},
};
use crate::model::id::NodeId;

/// Done tasks' learned estimates, as `Core` read them (#60).
#[derive(Debug, Clone, Default)]
pub struct Frozen(HashMap<NodeId, Option<Estimate>>);

impl Frozen {
    pub fn insert(&mut self, task: NodeId, learned: Option<Estimate>) {
        self.0.insert(task, learned);
    }

    /// `None`: not read for this task (`of_node` replays); `Some(None)`:
    /// read, nothing to learn from.
    pub fn get(&self, task: NodeId) -> Option<Option<Estimate>> {
        self.0.get(&task).copied()
    }
}

/// What udo would have estimated for `task` at its `frozen_at`, computed on
/// the history cut back to then. Costs a copy of the history: only for
/// tasks without a fitting row.
pub fn replay(task: NodeId, history: &History) -> Option<Estimate> {
    let at = history.task(task)?.frozen_at();
    let then = Average.estimate(task, &history.as_of(at))?;
    Some(Estimate {
        as_of: Some(at),
        ..then
    })
}

/// `task`'s frozen learned estimate from its rows (oldest first; other
/// tasks' rows are skipped): the latest learned (not `manual`) `created` /
/// `started` row, without sessions the latest `created` one. `None`: no
/// such row, or it says no source (before #60): replay it.
pub fn from_rows(rows: &[Recorded], task: &TaskRecord) -> Option<Estimate> {
    let counts = |reason: Reason| match task.first_session {
        Some(_) => reason.freezes(),
        None => reason == Reason::Created, // no session left: as created
    };
    let row = rows
        .iter()
        .rev()
        .map(|r| &r.estimate)
        .find(|e| e.task == task.id && e.method != "manual" && counts(e.reason))?;
    learned_from(row, task)
}

/// The `Estimate` a row says, as of `task`'s `frozen_at`.
fn learned_from(row: &NewEstimate, task: &TaskRecord) -> Option<Estimate> {
    let prior = match (row.prior, row.prior_source?) {
        (_, PriorSource::NoPrior) => None,
        (Some(minutes), PriorSource::Setting { container }) => {
            Some(Prior::Setting { container, minutes })
        }
        (Some(minutes), PriorSource::Parent { container, tasks }) => Some(Prior::Parent {
            container,
            tasks,
            minutes,
        }),
        (None, _) => return None, // a source without its minutes: no answer
    };
    let basis = match row.method.as_str() {
        "prior" => Basis::Prior(prior?),
        _ => Basis::Learned {
            container: task.container, // where it is now: rows do not keep it
            done_tasks: row.done_tasks,
            open_tasks: row.open_tasks,
            prior,
        },
    };
    Some(Estimate {
        minutes: row.minutes,
        basis,
        as_of: Some(task.frozen_at()),
    })
}

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    use super::*;
    use crate::{
        model::time::{Minutes, Time},
        test_util::at,
    };

    /// A done task, created at 8:00, first worked on at `first`.
    fn done(first: Option<Time>) -> TaskRecord {
        TaskRecord {
            id: NodeId::new(),
            container: NodeId::new(),
            created_at: at(8, 0),
            done_at: Some(at(13, 0)),
            actual: Minutes::new(30),
            first_session: first,
            own_estimate: None,
        }
    }

    /// A row of `task`: `minutes` by `method` for `reason`, learned from 2
    /// done and 1 open task, no prior.
    fn rec(task: &TaskRecord, reason: Reason, method: &str, minutes: u32) -> Recorded {
        Recorded {
            id: Uuid::now_v7(),
            estimate: NewEstimate {
                task: task.id,
                minutes: Minutes::new(minutes),
                method: method.into(),
                version: 1,
                done_tasks: 2,
                open_tasks: 1,
                prior: None,
                prior_source: Some(PriorSource::NoPrior),
                reason,
            },
            at: at(20, 0),
        }
    }

    /// `r` with the prior `minutes` from `source`.
    fn with_prior(mut r: Recorded, minutes: u32, source: PriorSource) -> Recorded {
        r.estimate.prior = Some(Minutes::new(minutes));
        r.estimate.prior_source = Some(source);
        r
    }

    #[test]
    fn a_learned_row_gives_its_estimate_and_basis() {
        // sheet-3: 3h05 from 2 done, 1 open, starting from the 3h set on uni
        let task = done(Some(at(12, 0)));
        let uni = NodeId::new();
        let row = rec(&task, Reason::Started, "average", 185);
        let row = with_prior(row, 180, PriorSource::Setting { container: uni });

        assert_eq!(
            from_rows(&[row], &task),
            Some(Estimate {
                minutes: Minutes::new(185),
                basis: Basis::Learned {
                    container: task.container,
                    done_tasks: 2,
                    open_tasks: 1,
                    prior: Some(Prior::Setting {
                        container: uni,
                        minutes: Minutes::new(180),
                    }),
                },
                as_of: Some(at(12, 0)), // the first session, not the row's `at`
            })
        );
    }

    #[test]
    fn a_prior_row_gives_the_prior_alone() {
        let task = done(None);
        let uni = NodeId::new();
        let row = rec(&task, Reason::Created, "prior", 96);
        let source = PriorSource::Parent {
            container: uni,
            tasks: 5,
        };
        let row = with_prior(row, 96, source);

        let estimate = from_rows(&[row], &task).unwrap();

        assert_eq!(
            estimate.basis,
            Basis::Prior(Prior::Parent {
                container: uni,
                tasks: 5,
                minutes: Minutes::new(96),
            })
        );
        assert_eq!(estimate.as_of, Some(at(8, 0))); // no session: its creation
    }

    /// The latest learned `created` / `started` row; without sessions the
    /// latest `created` one. `manual` and `cleared` rows never count.
    #[test]
    fn the_latest_created_or_started_learned_row_counts() {
        let worked = done(Some(at(12, 0)));
        let never = done(None);
        let rows = |t: &TaskRecord| {
            [
                rec(t, Reason::Created, "average", 60),
                rec(t, Reason::Created, "manual", 45),
                rec(t, Reason::Started, "average", 70),
                rec(t, Reason::Cleared, "average", 75),
            ]
        };

        let minutes = |t: &TaskRecord| from_rows(&rows(t), t).map(|e| e.minutes.get());

        assert_eq!(minutes(&worked), Some(70));
        assert_eq!(minutes(&never), Some(60));
    }

    #[test]
    fn no_fitting_row_is_none() {
        let task = done(None);
        let mut old = rec(&task, Reason::Created, "average", 60);
        old.estimate.prior_source = None; // written before #60
        let manual = rec(&task, Reason::Created, "manual", 45);

        assert_eq!(from_rows(&[], &task), None);
        assert_eq!(from_rows(&[manual], &task), None);
        assert_eq!(from_rows(&[old], &task), None);
    }

    #[test]
    fn rows_of_other_tasks_do_not_count() {
        let (task, other) = (done(None), done(None));

        let rows = [rec(&other, Reason::Created, "average", 60)];

        assert_eq!(from_rows(&rows, &task), None);
    }
}
