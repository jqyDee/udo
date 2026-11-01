//! Node: the task, its nearest project or workspace (else the root), or the
//! child of the scope the task lies under. A task that is no longer in the
//! tree is one removed-task key per task (level `task`), or one `Removed`
//! key for the coarser levels.

use std::{cell::RefCell, collections::HashMap};

use chrono::TimeZone;

use super::Dim;
use crate::{
    model::{
        NodePath,
        container::ContainerKind,
        id::NodeId,
        sessions::TaskRef,
        time::Time,
        tree::{Row, Tree},
    },
    stats::{Ctx, KeyPart, NodeKey, interval::Piece, spec::Level},
};

#[derive(Debug)]
pub struct Node {
    level: Level,
    /// The key of each task seen, by id. A run's dimensions see one tree and
    /// one scope, so the key of a task never changes within a run.
    keys: RefCell<HashMap<NodeId, NodeKey>>,
}

impl Node {
    pub fn new(level: Level) -> Self {
        Self {
            level,
            keys: RefCell::new(HashMap::new()),
        }
    }
}

impl<Tz: TimeZone> Dim<Tz> for Node {
    fn name(&self) -> &'static str {
        self.level.name()
    }

    fn cuts(&self, _from: Time, _to: Time, _tz: &Tz) -> Vec<Time> {
        Vec::new()
    }

    /// The tree is read once per distinct task (`node_key`), not per piece.
    fn key(&self, piece: &Piece<'_>, _from: Time, _to: Time, ctx: &Ctx<'_, Tz>) -> KeyPart {
        let task = &piece.interval.task;
        let mut keys = self.keys.borrow_mut();
        let key = keys
            .entry(task.id)
            .or_insert_with(|| node_key(self.level, task, ctx));
        KeyPart::Node(key.clone())
    }

    fn keys_in(&self, _from: Time, _to: Time, _tz: &Tz) -> Option<Vec<(KeyPart, u32)>> {
        None
    }
}

/// The key of a task at `level`. A task not in the tree is removed.
fn node_key<Tz: TimeZone>(level: Level, task: &TaskRef, ctx: &Ctx<'_, Tz>) -> NodeKey {
    let rows = ctx.tree.rows();
    let Some(row) = rows.iter().position(|r| r.node.id() == task.id) else {
        return match level {
            Level::Task => NodeKey::RemovedTask {
                id: task.id,
                name: task.name.clone(),
            },
            _ => NodeKey::Removed,
        };
    };
    let task_path = &rows[row].path;
    let target = match level {
        Level::Task => task_path.clone(),
        Level::Project => nearest(ctx.tree, task_path, ContainerKind::Project),
        Level::Workspace => nearest(ctx.tree, task_path, ContainerKind::Workspace),
        // The scope's child on the task's path; a task in the scope is itself.
        Level::Child => task_path[..(ctx.scope.len() + 1).min(task_path.len())].to_vec(),
    };
    node_key_at(ctx.tree, &rows, &target)
}

/// The path of the nearest strict ancestor of `path` that is a container of
/// `kind`; the root (`[]`) if there is none.
fn nearest(tree: &Tree, path: &[usize], kind: ContainerKind) -> NodePath {
    (0..path.len())
        .rev()
        .find(|&i| {
            tree.get(&path[..i])
                .and_then(|node| node.as_container())
                .is_some_and(|c| c.kind == kind)
        })
        .map_or_else(Vec::new, |i| path[..i].to_vec())
}

/// The key of the node at `target`, which is the root or a row of `rows`.
/// Order: the root first (0), then each row after it (its index + 1).
fn node_key_at(tree: &Tree, rows: &[Row<'_>], target: &[usize]) -> NodeKey {
    if target.is_empty() {
        return NodeKey::Node {
            id: tree.root.id(),
            path: String::new(),
            order: 0,
        };
    }
    let order = rows
        .iter()
        .position(|r| r.path == target)
        .expect("a task, its ancestor or the scope's child is a row");
    NodeKey::Node {
        id: tree.get(target).expect("the row exists").id(),
        path: tree.path_text(target),
        order: order + 1,
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use chrono_tz::Europe::Berlin;
    use serde_json::json;

    use super::*;
    use crate::{
        model::{container::ContainerKind, sessions::SessionId, tree::Tree},
        stats::{DimSpec, Interval, MeasureSpec, Query, Rows, SourceKind, run},
        test_util::{container_at, parse_time, task, tree_with, uni_tree},
    };

    /// A tracked interval, 09:00 to 10:00, on `task`.
    fn iv(task: TaskRef) -> Interval {
        Interval {
            start: parse_time("2026-10-27T09:00:00+01:00"),
            end: parse_time("2026-10-27T10:00:00+01:00"),
            task,
            session: Some(SessionId::new()),
            source: SourceKind::Tracked,
        }
    }

    /// A tracked interval on the task at `path` of `tree`.
    fn on(tree: &Tree, path: &[usize]) -> Interval {
        let node = tree.get(path).unwrap();
        iv(TaskRef {
            id: node.id(),
            name: node.name().into(),
            description: String::new(),
            container_dir: PathBuf::from("/uni"),
            container_id: NodeId::new(),
        })
    }

    /// A tracked interval on a task that is not in any tree.
    fn removed(name: &str) -> Interval {
        iv(TaskRef {
            id: NodeId::new(),
            name: name.into(),
            description: String::new(),
            container_dir: PathBuf::from("/uni"),
            container_id: NodeId::new(),
        })
    }

    /// `run` by `level` in Berlin, over October 2026, scoped to `scope`.
    fn rows(level: Level, tree: &Tree, scope: &[usize], intervals: &[Interval]) -> Rows {
        let query = Query {
            from: parse_time("2026-10-01T00:00:00+02:00"),
            to: parse_time("2026-11-01T00:00:00+01:00"),
            dims: vec![DimSpec::Node { level }],
            measures: vec![MeasureSpec::Minutes, MeasureSpec::Sessions],
            include_removed_tasks: true,
        };
        let ctx = Ctx {
            tree,
            scope,
            tz: Berlin,
        };
        run(&query, intervals, &ctx)
    }

    /// The text of each row's node key, in display order: the path ("" for
    /// the root), `name (removed)` for a removed task, `(removed)` otherwise.
    fn texts(rows: &Rows) -> Vec<String> {
        rows.rows
            .iter()
            .map(|row| match &row.key.0[..] {
                [KeyPart::Node(NodeKey::Node { path, .. })] => path.clone(),
                [KeyPart::Node(NodeKey::RemovedTask { name, .. })] => {
                    format!("{name} (removed)")
                }
                [KeyPart::Node(NodeKey::Removed)] => "(removed)".into(),
                other => panic!("not a node key: {other:?}"),
            })
            .collect()
    }

    #[test]
    fn task_level() {
        let tree = uni_tree();
        let rows = rows(Level::Task, &tree, &[], &[on(&tree, &[0, 0, 0])]);

        assert_eq!(texts(&rows), vec!["uni/cs/lab"]);
        assert_eq!(
            rows.rows[0].key.0,
            vec![KeyPart::Node(NodeKey::Node {
                id: tree.get(&[0, 0, 0]).unwrap().id(),
                path: "uni/cs/lab".into(),
                order: 3, // uni, cs, lab: the third row, 1-based
            })]
        );
    }

    #[test]
    fn project_nearest_kind_in_nested_containers() {
        let tree = tree_with(vec![container_at(
            "outer",
            Path::new("/tmp/outer"),
            ContainerKind::Project,
            vec![container_at(
                "inner",
                Path::new("/tmp/inner"),
                ContainerKind::Project,
                vec![task("t")],
            )],
        )]);
        let interval = [on(&tree, &[0, 0, 0])];

        assert_eq!(
            texts(&rows(Level::Project, &tree, &[], &interval)),
            vec!["outer/inner"]
        );
        // No workspace above the task: the root.
        assert_eq!(
            texts(&rows(Level::Workspace, &tree, &[], &interval)),
            vec![""]
        );
    }

    #[test]
    fn no_project_falls_back_to_root() {
        let tree = uni_tree();
        let rows = rows(Level::Project, &tree, &[], &[on(&tree, &[0, 0, 0])]);

        assert_eq!(texts(&rows), vec![""]);
        assert_eq!(
            rows.rows[0].key.0,
            vec![KeyPart::Node(NodeKey::Node {
                id: tree.root.id(),
                path: String::new(),
                order: 0,
            })]
        );
    }

    #[test]
    fn task_directly_in_root() {
        let tree = tree_with(vec![task("a")]);
        let interval = [on(&tree, &[0])];

        assert_eq!(texts(&rows(Level::Task, &tree, &[], &interval)), vec!["a"]);
        assert_eq!(
            texts(&rows(Level::Project, &tree, &[], &interval)),
            vec![""]
        );
        assert_eq!(
            texts(&rows(Level::Workspace, &tree, &[], &interval)),
            vec![""]
        );
    }

    #[test]
    fn child_of_scope() {
        let tree = uni_tree();
        let interval = [on(&tree, &[0, 0, 0])];

        // The scope is the root: the child is `uni`.
        assert_eq!(
            texts(&rows(Level::Child, &tree, &[], &interval)),
            vec!["uni"]
        );
        // The scope is `uni`: the child is `uni/cs`.
        assert_eq!(
            texts(&rows(Level::Child, &tree, &[0], &interval)),
            vec!["uni/cs"]
        );
        // The task is directly in the scope: itself.
        assert_eq!(
            texts(&rows(Level::Child, &tree, &[0, 0], &interval)),
            vec!["uni/cs/lab"]
        );
    }

    #[test]
    fn removed_task_rows() {
        let tree = uni_tree();
        let intervals = [removed("lab 3"), removed("lab 4")];

        let by_task = rows(Level::Task, &tree, &[], &intervals);
        assert_eq!(texts(&by_task), vec!["lab 3 (removed)", "lab 4 (removed)"]);

        let by_project = rows(Level::Project, &tree, &[], &intervals);
        assert_eq!(texts(&by_project), vec!["(removed)"]);
        assert_eq!(by_project.rows[0].values["minutes"], json!(120));
    }

    #[test]
    fn tree_order() {
        // Root: [z, b: [x], a: [y]]. Rows: z, a, y, b, x. Display: root, then
        // the containers in tree order, removed last.
        let tree = tree_with(vec![
            task("z"),
            container_at(
                "b",
                Path::new("/tmp/b"),
                ContainerKind::Project,
                vec![task("x")],
            ),
            container_at(
                "a",
                Path::new("/tmp/a"),
                ContainerKind::Project,
                vec![task("y")],
            ),
        ]);
        let intervals = [
            removed("gone"),
            on(&tree, &[1, 0]),
            on(&tree, &[2, 0]),
            on(&tree, &[0]),
        ];

        assert_eq!(
            texts(&rows(Level::Project, &tree, &[], &intervals)),
            vec!["", "a", "b", "(removed)"]
        );
    }
}
