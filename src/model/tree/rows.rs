//! Flat view of the tree: one row per node, depth-first. The CLI `List`
//! prints all of them; the TUI hides folded ones (`TreeState::rows`).

use crate::model::{
    NodePath,
    node::{
        Node,
        NodeBody::{self},
    },
    tree::Tree,
};

/// One line of the tree view.
pub struct Row<'a> {
    /// Indentation level; the root's children are depth 0.
    pub depth: usize,
    /// Path of the node (for `Tree::get`).
    pub path: NodePath,
    pub node: &'a Node,
}

impl Tree {
    /// All nodes below the root, depth-first (node, its children, next sibling).
    /// The root itself is not a row.
    pub fn rows(&self) -> Vec<Row<'_>> {
        self.rows_where(|_| true)
    }

    /// Like `rows`, but a node's children are only listed if `open(node)`.
    pub fn rows_where(&self, open: impl Fn(&Node) -> bool) -> Vec<Row<'_>> {
        let mut out = vec![];
        walk(&self.root, &open, &mut vec![], 0, &mut out);
        out
    }
}

/// Depth-first helper for `rows_where`: for each child `i` of `node`, push `i`
/// onto `path`, add a Row, recurse with `depth + 1` if `open`, pop `i`.
fn walk<'a>(
    node: &'a Node,
    open: &dyn Fn(&Node) -> bool,
    path: &mut NodePath,
    depth: usize,
    out: &mut Vec<Row<'a>>,
) {
    for (i, child) in sorted_tree(node.children()) {
        path.push(i);
        out.push(Row {
            depth,
            path: path.clone(),
            node: child,
        });
        if open(child) {
            walk(child, open, path, depth + 1, out);
        }
        path.pop();
    }
}

fn sorted_tree(children: &[Node]) -> Vec<(usize, &Node)> {
    let mut out: Vec<(usize, &Node)> = children.iter().enumerate().collect();

    // sort by NodeBody type (keep the indexes the same. This is just for visual)
    out.sort_by(|(_, a), (_, b)| match (&a.body, &b.body) {
        // check if tasks are done and move them to the bottom
        (NodeBody::Task(task_a), NodeBody::Task(task_b)) => {
            let a_is_done = task_a.done_at.is_some();
            let b_is_done = task_b.done_at.is_some();

            match a_is_done.cmp(&b_is_done) {
                std::cmp::Ordering::Equal => a.header.name.cmp(&b.header.name),
                ordering => ordering,
            }
        }
        (NodeBody::Task(_), NodeBody::Container(_)) => std::cmp::Ordering::Less,
        (NodeBody::Container(_), NodeBody::Task(_)) => std::cmp::Ordering::Greater,
        (NodeBody::Container(_), NodeBody::Container(_)) => a.header.name.cmp(&b.header.name),
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::{at, container, deep_tree, task, tree_with};

    fn names(rows: &[Row]) -> Vec<String> {
        rows.iter().map(|r| r.node.name().to_string()).collect()
    }

    // deep_tree: root: [a, inner: [b, deep: [c]], z, empty]; shown tasks
    // first: a, z, empty, inner: [b, deep: [c]]

    #[test]
    fn rows_are_depth_first() {
        let t = deep_tree();
        assert_eq!(
            names(&t.rows()),
            vec!["a", "z", "empty", "inner", "b", "deep", "c"]
        );
    }

    #[test]
    fn rows_have_depths() {
        let t = deep_tree();
        let depths: Vec<_> = t.rows().iter().map(|r| r.depth).collect();
        assert_eq!(depths, vec![0, 0, 0, 0, 1, 1, 2]);
    }

    /// Paths are the stored indices, not the shown positions: `get` finds
    /// the row's node.
    #[test]
    fn rows_have_paths_matching_get() {
        let t = deep_tree();
        let rows = t.rows();
        let paths: Vec<_> = rows.iter().map(|r| r.path.clone()).collect();
        assert_eq!(
            paths,
            vec![
                vec![0],
                vec![2],
                vec![3],
                vec![1],
                vec![1, 0],
                vec![1, 1],
                vec![1, 1, 0],
            ]
        );
        for r in &rows {
            assert_eq!(t.get(&r.path).unwrap().name(), r.node.name());
        }
    }

    /// Open tasks, then done tasks, then containers; each by name.
    #[test]
    fn rows_are_sorted_but_keep_the_stored_paths() {
        let mut t = tree_with(vec![
            container("y", vec![]),
            task("d"),
            container("x", vec![]),
            task("c"),
            task("b"),
        ]);
        if let NodeBody::Task(c) = &mut t.get_mut(&[3]).unwrap().body {
            c.done_at = Some(at(9, 0));
        }

        let rows = t.rows();

        assert_eq!(names(&rows), vec!["b", "d", "c", "x", "y"]);
        let paths: Vec<_> = rows.iter().map(|r| r.path.clone()).collect();
        assert_eq!(paths, vec![vec![4], vec![1], vec![3], vec![2], vec![0]]);
    }

    #[test]
    fn rows_empty_root() {
        assert!(tree_with(vec![]).rows().is_empty());
    }

    #[test]
    fn rows_where_skips_children_of_closed_nodes() {
        let t = deep_tree();
        let rows = t.rows_where(|n| n.name() != "inner");
        assert_eq!(names(&rows), vec!["a", "z", "empty", "inner"]);
    }
}
