//! `History`: the snapshot the estimators read, built once by `Core`.

use std::collections::HashMap;

use crate::model::{
    id::NodeId,
    node::{Node, NodeBody},
    sessions::Session,
    time::{Minutes, Time},
    tree::Tree,
};

/// One task as the estimators see it.
#[derive(Debug, Clone, PartialEq)]
pub struct TaskRecord {
    pub id: NodeId,
    /// The container it is in (deleted tasks: `TaskRef.container_id`).
    pub container: NodeId,
    /// The node's `created_at` (deleted tasks: their first session's start).
    pub created_at: Time,
    /// `None`: open, or deleted.
    pub done_at: Option<Time>,
    /// Sum of its sessions' minutes (a running one up to now).
    pub actual: Minutes,
    /// Start of its earliest session; `None`: never worked on.
    pub first_session: Option<Time>,
    /// Its own estimate (`Task::estimate`), for `Manual`; `None`: not set,
    /// or deleted (the task row is gone).
    pub own_estimate: Option<Minutes>,
}

impl TaskRecord {
    /// When the task existed from (`as_of`): its creation, or its first
    /// session if that lies before (work logged afterwards counts as of
    /// when it happened, #67).
    pub fn since(&self) -> Time {
        self.first_session
            .map_or(self.created_at, |first| first.min(self.created_at))
    }

    /// When a done task's estimate is frozen: its first session, also a
    /// back-dated one (#67), else its creation.
    pub fn frozen_at(&self) -> Time {
        self.first_session.unwrap_or(self.created_at)
    }
}

/// A container as the estimators see it.
#[derive(Debug, Clone, PartialEq)]
struct ContainerInfo {
    /// `None` at the root.
    parent: Option<NodeId>,
    /// The `estimate` set on this container itself, not the inherited one.
    own_estimate: Option<Minutes>,
}

/// Everything an estimator may look at. Owned: no borrow of `Tree`, so it
/// can be kept in the TUI's `App` and cut down with `as_of`.
#[derive(Debug, Clone, Default)]
pub struct History {
    tasks: Vec<TaskRecord>,
    /// `tasks` position by id, for `task`.
    index: HashMap<NodeId, usize>,
    containers: HashMap<NodeId, ContainerInfo>,
    /// Each task's sessions, for `as_of`.
    spans: HashMap<NodeId, Vec<Span>>,
}

impl History {
    /// From the tree and the (non-deleted) sessions; a running session
    /// counts up to `now`.
    pub fn build(tree: &Tree, sessions: &[Session], now: Time) -> Self {
        let mut sums: HashMap<NodeId, Sum> = HashMap::new();
        for s in sessions.iter().filter(|s| s.deleted_at.is_none()) {
            let span = Span {
                start: s.start,
                end: s.end.unwrap_or(now),
            };
            sums.entry(s.task.id)
                .or_insert_with(|| Sum {
                    spans: Vec::new(),
                    container: s.task.container_id,
                })
                .spans
                .push(span);
        }
        let mut history = Self::default();
        history.walk(&tree.root, None, &mut sums);

        // left over: tasks no longer in the tree (deleted); their sessions stay
        for (id, sum) in sums {
            if !history.containers.contains_key(&sum.container) {
                continue; // its container is gone too: nowhere to count it
            }
            let Some(first) = first_start(&sum.spans) else {
                continue; // never: a sum exists because of a session
            };
            history.tasks.push(TaskRecord {
                id,
                container: sum.container,
                created_at: first, // its creation is unknown: the first session
                done_at: None,
                actual: minutes(&sum.spans),
                first_session: Some(first),
                own_estimate: None, // was in the task row, gone with it
            });
            history.spans.insert(id, sum.spans);
        }
        history.tasks.sort_by_key(|t| (t.created_at, t.id));
        history.reindex();
        history
    }

    /// As it was at `t`: tasks that did not exist yet (`since` after `t`)
    /// dropped, every session cut at `t`, done only if done by `t` (a task
    /// done later is open then, with its time so far). Containers and their
    /// settings stay, and so do the tasks' own estimates: neither has a
    /// history.
    pub fn as_of(&self, t: Time) -> Self {
        let mut spans = HashMap::new();
        let tasks = self
            .tasks
            .iter()
            .filter(|r| r.since() <= t)
            .map(|r| {
                let cut: Vec<Span> = self
                    .spans
                    .get(&r.id)
                    .into_iter()
                    .flatten()
                    .filter(|s| s.start < t)
                    .map(|s| Span {
                        start: s.start,
                        end: s.end.min(t),
                    })
                    .collect();
                let record = TaskRecord {
                    actual: minutes(&cut),
                    first_session: first_start(&cut),
                    done_at: r.done_at.filter(|d| *d <= t),
                    ..r.clone()
                };
                spans.insert(r.id, cut);
                record
            })
            .collect();
        let mut h = Self {
            tasks,
            index: HashMap::new(),
            containers: self.containers.clone(),
            spans,
        };
        h.reindex();
        h
    }

    /// All tasks, oldest first (`created_at`, then id).
    pub fn tasks(&self) -> &[TaskRecord] {
        &self.tasks
    }

    /// The tasks directly in `container` (deleted ones included).
    pub fn tasks_in(&self, container: NodeId) -> impl Iterator<Item = &TaskRecord> {
        self.tasks.iter().filter(move |t| t.container == container)
    }

    pub fn task(&self, id: NodeId) -> Option<&TaskRecord> {
        self.index.get(&id).map(|&i| &self.tasks[i])
    }

    /// `None`: the root, or an unknown container.
    pub fn parent(&self, container: NodeId) -> Option<NodeId> {
        self.containers.get(&container).and_then(|c| c.parent)
    }

    /// The `estimate` set on `container` itself.
    pub fn own_estimate(&self, container: NodeId) -> Option<Minutes> {
        self.containers.get(&container).and_then(|c| c.own_estimate)
    }

    /// `container` is `ancestor` or somewhere inside it. Unknown containers
    /// are below nothing (but themselves).
    pub fn is_below(&self, container: NodeId, ancestor: NodeId) -> bool {
        let mut cur = Some(container);
        while let Some(c) = cur {
            if c == ancestor {
                return true;
            }
            cur = self.parent(c);
        }
        false
    }

    /// Rebuilds index from tasks; call after every change to tasks. This makes
    /// lookup O(1).
    fn reindex(&mut self) {
        self.index = self
            .tasks
            .iter()
            .enumerate()
            .map(|(i, t)| (t.id, i))
            .collect();
    }

    /// Containers into `containers`, tasks into `tasks`; takes each task's sum
    /// out of `sums`, so what is left belongs to deleted tasks.
    fn walk(&mut self, node: &Node, parent: Option<NodeId>, sums: &mut HashMap<NodeId, Sum>) {
        match &node.body {
            NodeBody::Task(t) => {
                let Some(container) = parent else {
                    return; // the root is always a container
                };
                let spans = sums.remove(&node.id()).map_or_else(Vec::new, |s| s.spans);
                self.tasks.push(TaskRecord {
                    id: node.id(),
                    container,
                    created_at: node.header.created_at,
                    done_at: t.done_at,
                    actual: minutes(&spans),
                    first_session: first_start(&spans),
                    own_estimate: t.estimate,
                });
                self.spans.insert(node.id(), spans);
            }
            NodeBody::Container(c) => {
                self.containers.insert(
                    node.id(),
                    ContainerInfo {
                        parent,
                        own_estimate: c.settings.estimate,
                    },
                );
                for child in &c.children {
                    self.walk(child, Some(node.id()), sums);
                }
            }
        }
    }
}

/// The sessions of one task, collected by `build`.
struct Sum {
    spans: Vec<Span>,
    container: NodeId,
}

/// One session as a time span (a running one ends at `now`).
#[derive(Debug, Clone, Copy, PartialEq)]
struct Span {
    start: Time,
    end: Time,
}

/// Whole minutes of `spans`, each rounded down on its own (like
/// `Session::duration`, so `udo show` and the estimates agree); a span
/// ending before its start counts 0.
fn minutes(spans: &[Span]) -> Minutes {
    let sum: i64 = spans
        .iter()
        .map(|s| (s.end - s.start).num_minutes().max(0))
        .sum();
    Minutes::new(u32::try_from(sum).unwrap_or(u32::MAX))
}

/// Start of the earliest span; `None`: no spans.
fn first_start(spans: &[Span]) -> Option<Time> {
    spans.iter().map(|s| s.start).min()
}
