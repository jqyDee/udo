//! Flat view of the tree: one row per node, depth-first. The CLI `List`
//! prints all of them; the TUI hides folded ones (`TreeState::rows`).

use crate::model::{NodePath, node::Node, tree::Tree};

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
    for (i, child) in node.children().iter().enumerate() {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::{deep_tree, tree_with};

    fn names(rows: &[Row]) -> Vec<String> {
        rows.iter().map(|r| r.node.name().to_string()).collect()
    }

    #[test]
    fn rows_are_depth_first() {
        let t = deep_tree();
        assert_eq!(
            names(&t.rows()),
            vec!["a", "inner", "b", "deep", "c", "z", "empty"]
        );
    }

    #[test]
    fn rows_have_depths() {
        let t = deep_tree();
        let depths: Vec<_> = t.rows().iter().map(|r| r.depth).collect();
        assert_eq!(depths, vec![0, 0, 1, 1, 2, 0, 0]);
    }

    #[test]
    fn rows_have_paths_matching_get() {
        let t = deep_tree();
        let rows = t.rows();
        let paths: Vec<_> = rows.iter().map(|r| r.path.clone()).collect();
        assert_eq!(
            paths,
            vec![
                vec![0],
                vec![1],
                vec![1, 0],
                vec![1, 1],
                vec![1, 1, 0],
                vec![2],
                vec![3],
            ]
        );
        for r in &rows {
            assert_eq!(t.get(&r.path).unwrap().name(), r.node.name());
        }
    }

    #[test]
    fn rows_empty_root() {
        assert!(tree_with(vec![]).rows().is_empty());
    }

    #[test]
    fn rows_where_skips_children_of_closed_nodes() {
        let t = deep_tree();
        let rows = t.rows_where(|n| n.name() != "inner");
        assert_eq!(names(&rows), vec!["a", "inner", "z", "empty"]);
    }
}
