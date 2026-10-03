//! Differential test: random sequences of operations run on the memory
//! store (the reference) and SQLite side by side. After every step both
//! must answer the same, hold the same sessions, and keep the invariants.
//! Finds the cases no contract check thought of; a failure shrinks to the
//! shortest sequence that still fails.
//!
//! A new backend: add a third `Side` and compare it the same way.

use std::path::PathBuf;

use chrono::{FixedOffset, TimeDelta, TimeZone};
use proptest::prelude::*;

use super::{memory::MemorySessions, rules::intersects, sqlite::SqliteSessions};
use crate::{
    model::{
        id::NodeId,
        sessions::{
            Owner, Session, SessionError, SessionId, SessionPatch, SessionQuery, SessionSource,
            SessionStore, TaskRef,
        },
        time::Time,
    },
    storage::{sqlite, time::time_to_sql},
};

const TASKS: usize = 3;
/// Time slots, 5 minutes apart from 08:00. Few of them, so equal times,
/// touching sessions and overlaps come up often.
const SLOTS: u16 = 60;
/// Sessions an operation can pick (by creation index, wrapping around).
const PICK: usize = 8;
/// Owners `start` / `stop` pick from: manual and two program instances, so
/// takeovers and foreign stops come up often.
const OWNERS: [&str; 3] = ["manual", "tmux:a", "tmux:b"];

/// Owner `i` with its source: `manual` by hand, the others from tmux.
fn owner(i: usize) -> (SessionSource, Owner) {
    let owner: Owner = OWNERS[i].parse().unwrap();
    let source = if owner.is_manual() {
        SessionSource::Manual
    } else {
        "tmux".parse().unwrap()
    };
    (source, owner)
}

/// One operation, with times as slots and sessions as creation indices (the
/// ids differ between the stores).
#[derive(Debug, Clone)]
enum Op {
    Start {
        task: usize,
        owner: usize,
        at: u16,
    },
    /// `owner: None`: a manual stop (stops anything).
    Stop {
        owner: Option<usize>,
        at: u16,
        west: bool,
    },
    Add {
        task: usize,
        start: u16,
        len: u16,
        west: bool,
    },
    Edit {
        session: usize,
        start: Option<u16>,
        end: Option<u16>,
    },
    Split {
        session: usize,
        at: u16,
    },
    Cut {
        session: usize,
        from: u16,
        to: u16,
    },
    Delete {
        session: usize,
    },
    Query {
        task: Option<usize>,
        from: Option<u16>,
        to: Option<u16>,
        include_deleted: bool,
    },
}

fn op() -> impl Strategy<Value = Op> {
    let slot = || 0..SLOTS;
    let task = || 0..TASKS;
    let session = || 0..PICK;
    let owner = || 0..OWNERS.len();
    prop_oneof![
        3 => (task(), owner(), slot()).prop_map(|(task, owner, at)| Op::Start { task, owner, at }),
        2 => (proptest::option::of(owner()), slot(), any::<bool>())
            .prop_map(|(owner, at, west)| Op::Stop { owner, at, west }),
        3 => (task(), slot(), 0..12u16, any::<bool>())
            .prop_map(|(task, start, len, west)| Op::Add { task, start, len, west }),
        2 => (session(), proptest::option::of(slot()), proptest::option::of(slot()))
            .prop_map(|(session, start, end)| Op::Edit { session, start, end }),
        2 => (session(), slot()).prop_map(|(session, at)| Op::Split { session, at }),
        2 => (session(), slot(), slot()).prop_map(|(session, from, to)| Op::Cut { session, from, to }),
        1 => session().prop_map(|session| Op::Delete { session }),
        2 => (
            proptest::option::of(task()),
            proptest::option::of(slot()),
            proptest::option::of(slot()),
            any::<bool>(),
        )
            .prop_map(|(task, from, to, include_deleted)| Op::Query {
                task,
                from,
                to,
                include_deleted,
            }),
    ]
}

/// Slot `n` as a time: 2026-10-15 08:00 + 5 min × `n`, at +02:00, or the
/// same instant at -04:00 (`west`, an end recorded somewhere else).
fn slot(n: u16, west: bool) -> Time {
    let t = FixedOffset::east_opt(2 * 3600)
        .unwrap()
        .with_ymd_and_hms(2026, 10, 15, 8, 0, 0)
        .unwrap()
        + TimeDelta::minutes(5 * i64::from(n));
    if west {
        t.with_timezone(&FixedOffset::west_opt(4 * 3600).unwrap())
    } else {
        t
    }
}

/// Both stores' clock: 20:00, after every slot.
fn now() -> Time {
    slot(144, false)
}

/// A session with everything store-specific taken out: ids become creation
/// indices, times become (ms, offset) so offsets are compared too.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Norm {
    start: (i64, i32),
    session: Option<usize>,
    task: Option<usize>,
    end: Option<(i64, i32)>,
    source: String,
    owner: String,
    created: (i64, i32),
    edited: Option<(i64, i32)>,
    deleted: Option<(i64, i32)>,
}

/// One store plus the sessions it created, in creation order: the same index
/// means the same session in both stores.
struct Side<S> {
    store: S,
    known: Vec<SessionId>,
}

impl<S: SessionStore> Side<S> {
    fn new(store: S) -> Self {
        Self {
            store,
            known: Vec::new(),
        }
    }

    /// The session at creation index `i` (wrapping); none created yet: an
    /// unknown id.
    fn id(&self, i: usize) -> SessionId {
        match self.known.len() {
            0 => SessionId::new(),
            n => self.known[i % n],
        }
    }

    fn learn(&mut self, sessions: &[Session]) {
        for s in sessions {
            if !self.known.contains(&s.id) {
                self.known.push(s.id);
            }
        }
    }

    /// Sorted by start, then creation index: the order of equal starts is
    /// not part of the contract.
    fn norm(&self, sessions: &[Session], tasks: &[TaskRef]) -> Vec<Norm> {
        let mut out: Vec<_> = sessions
            .iter()
            .map(|s| Norm {
                start: time_to_sql(s.start),
                session: self.known.iter().position(|id| *id == s.id),
                task: tasks.iter().position(|t| t.id == s.task.id),
                end: s.end.map(time_to_sql),
                source: s.source.to_string(),
                owner: s.owner.to_string(),
                created: time_to_sql(s.created_at),
                edited: s.edited_at.map(time_to_sql),
                deleted: s.deleted_at.map(time_to_sql),
            })
            .collect();
        out.sort();
        out
    }

    /// Run `op`; the sessions it returned, normalized.
    async fn apply(&mut self, op: &Op, tasks: &[TaskRef]) -> Result<Vec<Norm>, SessionError> {
        let at = |n| slot(n, false);
        let returned: Vec<Session> = match *op {
            Op::Start {
                task,
                owner: o,
                at: n,
            } => {
                let (source, owner) = owner(o);
                vec![
                    self.store
                        .start(tasks[task].clone(), source, owner, at(n))
                        .await?,
                ]
            }
            Op::Stop {
                owner: o,
                at: n,
                west,
            } => {
                let owner = o.map(|o| owner(o).1);
                let stopped = self.store.stop(owner.as_ref(), slot(n, west)).await?;
                stopped.into_iter().collect()
            }
            Op::Add {
                task,
                start,
                len,
                west,
            } => {
                let end = slot(start + len, west);
                vec![self.store.add(tasks[task].clone(), at(start), end).await?]
            }
            Op::Edit {
                session,
                start,
                end,
            } => {
                let patch = SessionPatch {
                    start: start.map(at),
                    end: end.map(at),
                };
                self.store.edit(self.id(session), patch).await?;
                vec![]
            }
            Op::Split { session, at: n } => {
                let pieces = self.store.split(self.id(session), at(n)).await?;
                pieces.map_or(vec![], |(a, b)| vec![a, b])
            }
            Op::Cut { session, from, to } => {
                self.store.cut(self.id(session), at(from), at(to)).await?
            }
            Op::Delete { session } => {
                self.store.delete(self.id(session)).await?;
                vec![]
            }
            Op::Query {
                task,
                from,
                to,
                include_deleted,
            } => {
                let q = SessionQuery {
                    tasks: task.map(|t| vec![tasks[t].id]),
                    from: from.map(at),
                    to: to.map(at),
                    include_deleted,
                };
                self.store.query(&q).await?
            }
        };
        self.learn(&returned);
        Ok(self.norm(&returned, tasks))
    }

    /// Every session, deleted ones too, as the store returns them.
    async fn everything(&self) -> Vec<Session> {
        let all = SessionQuery {
            include_deleted: true,
            ..Default::default()
        };
        self.store.query(&all).await.unwrap()
    }
}

/// What must hold after any sequence of operations, in any backend.
fn check_invariants(all: &[Session], which: &str, step: usize, op: &Op) {
    let at = format!("{which}, after step {step} ({op:?})");
    let visible: Vec<_> = all.iter().filter(|s| s.deleted_at.is_none()).collect();

    let running = visible.iter().filter(|s| s.end.is_none()).count();
    assert!(running <= 1, "{running} sessions running: {at}");
    for s in all {
        assert!(
            s.end.is_none_or(|end| end > s.start),
            "end not after start: {s:?}: {at}"
        );
    }
    for (i, a) in visible.iter().enumerate() {
        for b in &visible[i + 1..] {
            assert!(
                !intersects(a.start, a.end, Some(b.start), b.end),
                "overlap: {a:?} and {b:?}: {at}"
            );
        }
    }
    assert!(
        all.windows(2).all(|w| w[0].start <= w[1].start),
        "not sorted by start: {at}"
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    #[test]
    fn memory_and_sqlite_agree(ops in proptest::collection::vec(op(), 1..40)) {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        rt.block_on(async {
            let tasks: Vec<TaskRef> = (0..TASKS)
                .map(|i| TaskRef {
                    id: NodeId::new(),
                    name: format!("task {i}"),
                    description: String::new(),
                    container_dir: PathBuf::from("uni"),
                    container_id: NodeId::new(),
                })
                .collect();
            let mut memory = Side::new(MemorySessions::new(now));
            let mut sqlite = Side::new(SqliteSessions::new(sqlite::open_in_memory().unwrap(), now));

            for (step, op) in ops.iter().enumerate() {
                let (a, b) = (memory.apply(op, &tasks).await, sqlite.apply(op, &tasks).await);
                assert_eq!(a, b, "different answers at step {step} ({op:?})");

                let (all_a, all_b) = (memory.everything().await, sqlite.everything().await);
                check_invariants(&all_a, "memory", step, op);
                check_invariants(&all_b, "sqlite", step, op);
                assert_eq!(
                    memory.norm(&all_a, &tasks),
                    sqlite.norm(&all_b, &tasks),
                    "different sessions after step {step} ({op:?})"
                );

                let running_a = memory.store.running().await.unwrap();
                let running_b = sqlite.store.running().await.unwrap();
                assert_eq!(
                    memory.norm(running_a.as_slice(), &tasks),
                    sqlite.norm(running_b.as_slice(), &tasks),
                    "different running session after step {step} ({op:?})"
                );
            }
        });
    }
}
