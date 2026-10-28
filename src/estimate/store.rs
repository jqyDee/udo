//! The estimate history: what udo estimated for a task, at the moments it
//! matters. Append-only; backends in `storage::estimates`.

use std::{fmt, str::FromStr};

use uuid::Uuid;

use crate::{
    estimate::{Basis, Estimate, Prior},
    model::{
        id::NodeId,
        time::{Minutes, Time, to_ms},
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
    /// Your own estimate was set or changed (#6).
    Manual,
    /// Your own estimate was removed: the row is what udo goes by then,
    /// the learned one (#6).
    Cleared,
}

impl Reason {
    /// A row a done task's estimate is read from (#60): the moments it is
    /// frozen at.
    pub fn freezes(self) -> bool {
        matches!(self, Self::Created | Self::Started)
    }
}

/// Where a row's prior came from (#60), so the row alone gives the whole
/// estimate, explanation included.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PriorSource {
    /// No prior (and yours: `manual` rows).
    NoPrior,
    /// The `estimate` setting of `container`.
    Setting { container: NodeId },
    /// `tasks` tasks pooled in `container`.
    Parent { container: NodeId, tasks: usize },
}

impl PriorSource {
    fn of(prior: Option<Prior>) -> Self {
        match prior {
            None => Self::NoPrior,
            Some(Prior::Setting { container, .. }) => Self::Setting { container },
            Some(Prior::Parent {
                container, tasks, ..
            }) => Self::Parent { container, tasks },
        }
    }
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
    /// `None`: written before #60 (migration 003), unknown.
    pub prior_source: Option<PriorSource>,
    /// The moment a learned row was computed for (#60): the task's
    /// `frozen_at` then, to the millisecond (`to_ms`). A done task's
    /// estimate is read from the row for its current `frozen_at`. `None`:
    /// yours (never frozen), not a moment (`manual`, `cleared`), or
    /// written before #60.
    pub frozen_at: Option<Time>,
    pub reason: Reason,
}

impl NewEstimate {
    /// `estimate` of `task` by the estimator `method` / `version`, computed
    /// for the moment `frozen_at` (`None` for `manual` / `cleared`; yours
    /// never keeps one).
    pub fn of(
        task: NodeId,
        estimate: &Estimate,
        method: &str,
        version: u32,
        reason: Reason,
        frozen_at: Option<Time>,
    ) -> Self {
        let (method, done_tasks, open_tasks, prior) = match estimate.basis {
            Basis::Own => (method, 0, 0, None),
            Basis::Prior(p) => ("prior", 0, 0, Some(p)),
            Basis::Learned {
                done_tasks,
                open_tasks,
                prior,
                ..
            } => (method, done_tasks, open_tasks, prior),
        };
        Self {
            task,
            minutes: estimate.minutes,
            method: method.into(),
            version,
            done_tasks,
            open_tasks,
            prior: prior.map(|p| p.minutes()),
            prior_source: Some(PriorSource::of(prior)),
            frozen_at: match estimate.basis {
                Basis::Own => None,        // yours: the current value, never frozen
                _ => frozen_at.map(to_ms), // as the database keeps it
            },
            reason,
        }
    }

    /// A learned row for a moment a done task is read from (`created`,
    /// `started`; #60), not one of yours.
    fn is_moment(&self) -> bool {
        self.method != "manual" && self.reason.freezes()
    }

    /// Do this row and `other` belong to one chain, the rows `repeats`
    /// compares within (#60): two chains, the learned moments (`created`,
    /// `started`), and the rest: all of yours (whatever the reason: never
    /// frozen) with the `cleared` ones between them, what udo goes by after
    /// an edit. So a `manual` row never hides a learned moment, a `cleared`
    /// one never a `started` one, your estimate set again after removing
    /// it is a new row, and a task started with your unchanged estimate
    /// is one.
    pub fn same_chain(&self, other: &NewEstimate) -> bool {
        self.is_moment() == other.is_moment()
    }

    /// Would this row repeat `last` (the latest of its chain): same
    /// minutes, method, prior source and moment? Then it says nothing new
    /// (the reason alone does not count). A row without a source or a
    /// moment (before #60) is never repeated by one with it; nor is a row
    /// for another moment (a task started with the estimate it was created
    /// with: a row for each, #60).
    pub fn repeats(&self, last: &NewEstimate) -> bool {
        self.minutes == last.minutes
            && self.method == last.method
            && self.prior_source == last.prior_source
            && self.frozen_at == last.frozen_at
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
    /// latest row of its chain (`same_chain`, `repeats`): `None`, nothing
    /// written. Check and write are one step, also across processes, so
    /// two at once write it once.
    async fn record(&self, row: NewEstimate) -> Result<Option<Recorded>, EstimateError>;
    /// The task's latest row written. Not necessarily what udo goes by:
    /// yours and the learned one are written side by side (#60).
    async fn last_of(&self, task: NodeId) -> Result<Option<Recorded>, EstimateError>;
    /// The rows of `tasks`, oldest first (#9, #12).
    async fn of_tasks(&self, tasks: &[NodeId]) -> Result<Vec<Recorded>, EstimateError>;
}

impl fmt::Display for Reason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Created => "created",
            Self::Started => "started",
            Self::Manual => "manual",
            Self::Cleared => "cleared",
        })
    }
}

impl FromStr for Reason {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "created" => Ok(Self::Created),
            "started" => Ok(Self::Started),
            "manual" => Ok(Self::Manual),
            "cleared" => Ok(Self::Cleared),
            other => Err(format!("unknown estimate reason: {other}")),
        }
    }
}

impl std::error::Error for EstimateError {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        estimate::Prior,
        test_util::{at, parse_time},
    };

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
        let uni = NodeId::new();
        let prior = Prior::Setting {
            container: uni,
            minutes: Minutes::new(180),
        };

        let row = NewEstimate::of(
            task,
            &learned(0, 1, Some(prior)),
            "average",
            1,
            Reason::Started,
            Some(at(12, 0)),
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
                prior_source: Some(PriorSource::Setting { container: uni }),
                frozen_at: Some(at(12, 0)),
                reason: Reason::Started,
            }
        );
    }

    /// The moment is kept as the database keeps it (to the millisecond:
    /// equal to itself read back); yours keeps none, never frozen.
    #[test]
    fn a_row_keeps_its_moment_to_the_millisecond_yours_none() {
        let moment = parse_time("2026-10-03T23:53:37.843188+02:00"); // as in a `.udo.toml`
        let own = Estimate {
            minutes: Minutes::new(45),
            basis: Basis::Own,
            as_of: None,
        };
        let of = |e: &Estimate| {
            NewEstimate::of(NodeId::new(), e, "x", 1, Reason::Created, Some(moment)).frozen_at
        };

        assert_eq!(
            of(&learned(2, 0, None)),
            Some(parse_time("2026-10-03T23:53:37.843+02:00"))
        );
        assert_eq!(of(&own), None);
    }

    /// Two chains: the learned moments (`created`, `started`), and the
    /// rest: yours whatever the reason, with the `cleared` ones.
    #[test]
    fn chains_the_learned_moments_and_the_rest() {
        let row = |method: &str, reason| NewEstimate {
            method: method.into(),
            reason,
            ..NewEstimate::of(
                NodeId::new(),
                &learned(2, 0, None),
                "average",
                1,
                reason,
                None,
            )
        };
        let chain = |a: &NewEstimate, b: &NewEstimate| a.same_chain(b);

        assert!(chain(
            &row("manual", Reason::Manual),
            &row("manual", Reason::Started)
        ));
        assert!(chain(
            &row("average", Reason::Created),
            &row("prior", Reason::Started)
        ));
        assert!(!chain(
            &row("average", Reason::Started),
            &row("average", Reason::Cleared)
        ));
        assert!(!chain(
            &row("manual", Reason::Started),
            &row("average", Reason::Started)
        ));
        assert!(chain(
            &row("manual", Reason::Manual),
            &row("average", Reason::Cleared)
        ));
    }

    #[test]
    fn a_row_keeps_where_its_prior_came_from() {
        let ws = NodeId::new();
        let setting = Prior::Setting {
            container: ws,
            minutes: Minutes::new(180),
        };
        let parent = Prior::Parent {
            container: ws,
            tasks: 5,
            minutes: Minutes::new(96),
        };
        let prior_only = Estimate {
            minutes: Minutes::new(96),
            basis: Basis::Prior(parent),
            as_of: None,
        };
        let own = Estimate {
            minutes: Minutes::new(45),
            basis: Basis::Own,
            as_of: None,
        };
        let source = |e: &Estimate| {
            NewEstimate::of(NodeId::new(), e, "average", 1, Reason::Created, None).prior_source
        };

        assert_eq!(
            source(&learned(0, 1, Some(setting))),
            Some(PriorSource::Setting { container: ws })
        );
        assert_eq!(
            source(&prior_only),
            Some(PriorSource::Parent {
                container: ws,
                tasks: 5
            })
        );
        assert_eq!(source(&learned(2, 0, None)), Some(PriorSource::NoPrior));
        assert_eq!(source(&own), Some(PriorSource::NoPrior));
    }

    #[test]
    fn created_and_started_rows_freeze() {
        assert!(Reason::Created.freezes() && Reason::Started.freezes());
        assert!(!Reason::Manual.freezes() && !Reason::Cleared.freezes());
    }

    #[test]
    fn a_learned_estimate_without_a_prior_has_none() {
        let row = NewEstimate::of(
            NodeId::new(),
            &learned(2, 0, None),
            "average",
            1,
            Reason::Created,
            None,
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

        let row = NewEstimate::of(
            NodeId::new(),
            &estimate,
            "average",
            1,
            Reason::Created,
            None,
        );

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
