//! The estimate history: what udo estimated for a task, at the moments it
//! matters. Append-only; backends in `storage::estimates`.

use std::{fmt, str::FromStr};

use uuid::Uuid;

use crate::{
    estimate::{Basis, Estimate},
    model::{
        id::NodeId,
        time::{Minutes, Time},
    },
};

/// Why a row was written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    /// The task was created.
    Created,
    /// The task got its first session (the moment a done task's estimate
    /// is frozen at).
    Started,
}

/// A row to append; the store adds the id and the time.
#[derive(Debug, Clone, PartialEq)]
pub struct NewEstimate {
    pub task: NodeId,
    pub minutes: Minutes,
    /// `prior` for an estimate from the prior alone, else the estimator's.
    pub method: String,
    pub version: u32,
    pub done_tasks: usize,
    pub open_tasks: usize,
    pub prior: Option<Minutes>,
    pub reason: Reason,
}

impl NewEstimate {
    /// `estimate` of `task` by the estimator `method` / `version`.
    pub fn of(
        task: NodeId,
        estimate: &Estimate,
        method: &str,
        version: u32,
        reason: Reason,
    ) -> Self {
        let (method, done_tasks, open_tasks, prior) = match estimate.basis {
            Basis::Prior(p) => ("prior", 0, 0, Some(p.minutes())),
            Basis::Learned {
                done_tasks,
                open_tasks,
                prior,
                ..
            } => (method, done_tasks, open_tasks, prior.map(|p| p.minutes())),
        };
        Self {
            task,
            minutes: estimate.minutes,
            method: method.into(),
            version,
            done_tasks,
            open_tasks,
            prior,
            reason,
        }
    }

    /// Would this row repeat `last`: same minutes, same method? Then it says
    /// nothing new (the reason alone does not count: a task started with
    /// the estimate it was created with gets no second row).
    pub fn repeats(&self, last: &NewEstimate) -> bool {
        self.minutes == last.minutes && self.method == last.method
    }
}

/// A stored row.
#[derive(Debug, Clone, PartialEq)]
pub struct Recorded {
    pub id: Uuid,
    pub estimate: NewEstimate,
    /// Store clock.
    pub at: Time,
}

#[derive(Debug)] // or by hand, as SessionError does
pub enum EstimateError {
    Backend(String),
}

impl fmt::Display for EstimateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Backend(e) => write!(f, "estimate store error: {e}"),
        }
    }
}

#[allow(async_fn_in_trait)]
pub trait EstimateStore {
    /// Append `row`, timed by the store's clock, unless it repeats the
    /// task's latest row (`repeats`): `None`, nothing written. Check and
    /// write are one step, also across processes, so two at once write it
    /// once.
    async fn record(&self, row: NewEstimate) -> Result<Option<Recorded>, EstimateError>;
    /// The task's latest row (#9: what udo estimated last).
    async fn last_of(&self, task: NodeId) -> Result<Option<Recorded>, EstimateError>;
    /// The rows of `tasks`, oldest first (#9, #12).
    async fn of_tasks(&self, tasks: &[NodeId]) -> Result<Vec<Recorded>, EstimateError>;
}

impl fmt::Display for Reason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Created => "created",
            Self::Started => "started",
        })
    }
}

impl FromStr for Reason {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "created" => Ok(Self::Created),
            "started" => Ok(Self::Started),
            other => Err(format!("unknown estimate reason: {other}")),
        }
    }
}

impl std::error::Error for EstimateError {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::estimate::Prior;

    fn learned(done_tasks: usize, open_tasks: usize, prior: Option<Prior>) -> Estimate {
        Estimate {
            minutes: Minutes::new(185),
            basis: Basis::Learned {
                container: NodeId::new(),
                done_tasks,
                open_tasks,
                prior,
            },
            as_of: None,
        }
    }

    #[test]
    fn a_learned_estimate_keeps_its_method_counts_and_prior() {
        // uni/algorithms/sheet-3 at its first session: 3h05 from one open
        // sheet, starting from the 3h setting
        let task = NodeId::new();
        let prior = Prior::Setting {
            container: NodeId::new(),
            minutes: Minutes::new(180),
        };

        let row = NewEstimate::of(
            task,
            &learned(0, 1, Some(prior)),
            "average",
            1,
            Reason::Started,
        );

        assert_eq!(
            row,
            NewEstimate {
                task,
                minutes: Minutes::new(185),
                method: "average".into(),
                version: 1,
                done_tasks: 0,
                open_tasks: 1,
                prior: Some(Minutes::new(180)),
                reason: Reason::Started,
            }
        );
    }

    #[test]
    fn a_learned_estimate_without_a_prior_has_none() {
        let row = NewEstimate::of(
            NodeId::new(),
            &learned(2, 0, None),
            "average",
            1,
            Reason::Created,
        );

        assert_eq!((row.done_tasks, row.open_tasks, row.prior), (2, 0, None));
    }

    #[test]
    fn an_estimate_from_the_prior_alone_is_method_prior() {
        // nothing learned yet: the estimate is the prior, whatever the
        // estimator; its pooled tasks belong to the prior, not to the row
        let estimate = Estimate {
            minutes: Minutes::new(96),
            basis: Basis::Prior(Prior::Parent {
                container: NodeId::new(),
                tasks: 5,
                minutes: Minutes::new(96),
            }),
            as_of: None,
        };

        let row = NewEstimate::of(NodeId::new(), &estimate, "average", 1, Reason::Created);

        assert_eq!(row.method, "prior");
        assert_eq!((row.done_tasks, row.open_tasks), (0, 0));
        assert_eq!(row.prior, Some(Minutes::new(96)));
        assert_eq!(row.minutes, Minutes::new(96));
    }

    #[test]
    fn the_error_names_the_store() {
        let err = EstimateError::Backend("disk full".into());

        assert_eq!(err.to_string(), "estimate store error: disk full");
    }
}
