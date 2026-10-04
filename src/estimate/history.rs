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
/// can be kept in the TUI's `App` and cut down with `as_of` later.
#[derive(Debug, Clone, Default)]
pub struct History {
    tasks: Vec<TaskRecord>,
    containers: HashMap<NodeId, ContainerInfo>,
}

impl History {
    /// From the tree and the (non-deleted) sessions; a running session
    /// counts up to `now`.
    pub fn build(tree: &Tree, sessions: &[Session], now: Time) -> Self {
        let mut sums: HashMap<NodeId, Sum> = HashMap::new();
        for s in sessions.iter().filter(|s| s.deleted_at.is_none()) {
            let minutes = s.duration(now).get();
            sums.entry(s.task.id)
                .and_modify(|sum| {
                    sum.minutes += minutes;
                    sum.first_start = sum.first_start.min(s.start);
                })
                .or_insert(Sum {
                    minutes,
                    first_start: s.start,
                    container: s.task.container_id,
                });
        }
        let mut history = Self::default();
        history.walk(&tree.root, None, &mut sums);

        // left over: tasks no longer in the tree (deleted); their sessions stay
        for (id, sum) in sums {
            if !history.containers.contains_key(&sum.container) {
                continue; // its container is gone too: nowhere to count it
            }
            history.tasks.push(TaskRecord {
                id,
                container: sum.container,
                created_at: sum.first_start,
                done_at: None,
                actual: Minutes::new(sum.minutes),
            });
        }
        history.tasks.sort_by_key(|t| (t.created_at, t.id));
        history
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
        self.tasks().iter().find(|t| t.id == id)
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

    /// Containers into `containers`, tasks into `tasks`; takes each task's sum
    /// out of `sums`, so what is left belongs to deleted tasks.
    fn walk(&mut self, node: &Node, parent: Option<NodeId>, sums: &mut HashMap<NodeId, Sum>) {
        match &node.body {
            NodeBody::Task(t) => {
                let Some(container) = parent else {
                    return; // the root is always a container
                };
                let actual = sums.remove(&node.id()).map_or(0, |s| s.minutes);
                self.tasks.push(TaskRecord {
                    id: node.id(),
                    container,
                    created_at: node.header.created_at,
                    done_at: t.done_at,
                    actual: Minutes::new(actual),
                });
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

/// What the sessions of one task add up to.
struct Sum {
    minutes: u32,
    first_start: Time,
    container: NodeId,
}
