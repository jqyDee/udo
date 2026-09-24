//! Flat view of the tree + cursor movement. Shared by CLI `List` and the TUI.

use crate::model::{
    node::Node,
    tree::{NodePath, Tree},
};

/// One visible line of the tree view.
pub struct Row<'a> {
    /// Indentation level; the root's children are depth 0.
    pub depth: usize,
    /// Path of the node; `row.path == tree.cursor` means "selected".
    pub path: NodePath,
    pub node: &'a Node,
}

impl Tree {
    /// All nodes below the root, depth-first (node, its children, next sibling).
    /// The root itself is not a row.
    pub fn rows(&self) -> Vec<Row<'_>> {
        let mut out = vec![];
        walk(&self.root, &mut vec![], 0, &mut out);
        out
    }

    /// Select the next row. Stays on the last row.
    /// Cursor not on any row (e.g. `[]` after load) -> first row.
    pub fn move_down(&mut self) {
        self.step(true);
    }

    /// Select the previous row. Stays on the first row.
    /// Cursor not on any row -> first row.
    pub fn move_up(&mut self) {
        self.step(false);
    }

    /// Go to the first child, if the cursor is on a container with children.
    /// A collapsed container is expanded first.
    pub fn move_in(&mut self) {
        if self.cursor.is_empty() {
            return; // `get(&[])` is the root, which is not a row
        }
        let has_children = self
            .get(&self.cursor)
            .is_some_and(|n| !n.children().is_empty());
        if has_children {
            self.expand();
            self.cursor.push(0);
        }
    }

    /// Hide the children of the container at the cursor. On a task or an empty
    /// container, the parent is collapsed instead and the cursor moves onto it,
    /// so the cursor is never hidden. No-op on top-level tasks.
    pub fn collapse(&mut self) {
        if self.cursor.is_empty() {
            return;
        }
        let has_children = self
            .get(&self.cursor)
            .is_some_and(|n| !n.children().is_empty());
        if !has_children {
            if self.cursor.len() == 1 {
                return; // parent is the root, which can't be collapsed
            }
            self.cursor.pop();
        }
        let path = self.cursor.clone();
        self.set_collapsed(&path, true);
    }

    /// Show the children of the container at the cursor again.
    pub fn expand(&mut self) {
        if self.cursor.is_empty() {
            return;
        }
        let path = self.cursor.clone();
        self.set_collapsed(&path, false);
    }

    /// Collapse or expand the container at the cursor. No-op on tasks.
    pub fn toggle_collapse(&mut self) {
        if self.cursor.is_empty() {
            return;
        }
        let Some(Node::Container(c)) = self.get(&self.cursor) else {
            return;
        };
        let collapsed = !c.collapsed;
        let path = self.cursor.clone();
        self.set_collapsed(&path, collapsed);
    }

    /// Collapse every container. The cursor moves to its top-level ancestor,
    /// the only rows still visible.
    pub fn collapse_all(&mut self) {
        set_collapsed_all(&mut self.root, true);
        self.cursor.truncate(1);
    }

    /// Expand every container.
    pub fn expand_all(&mut self) {
        set_collapsed_all(&mut self.root, false);
    }

    fn set_collapsed(&mut self, path: &[usize], collapsed: bool) {
        if let Some(Node::Container(c)) = self.get_mut(path) {
            c.collapsed = collapsed;
        }
    }

    /// Go to the parent. Never above the root's children (depth 0).
    pub fn move_out(&mut self) {
        if self.cursor.len() > 1 {
            self.cursor.pop();
        }
    }

    /// One row down (`down`) or up, clamped to the ends.
    fn step(&mut self, down: bool) {
        let rows = self.rows();
        if rows.is_empty() {
            return;
        }
        let next = match rows.iter().position(|r| r.path == self.cursor) {
            None => 0,
            Some(i) if down => (i + 1).min(rows.len() - 1),
            Some(i) => i.saturating_sub(1),
        };
        // clone first: `rows` borrows `self`, the borrow ends after this line
        let path = rows[next].path.clone();
        self.cursor = path;
    }
}

/// Depth-first helper for `rows`: for each child `i` of `node`, push `i` onto
/// `path`, add a Row, recurse with `depth + 1` (unless collapsed), pop `i`.
fn walk<'a>(node: &'a Node, path: &mut NodePath, depth: usize, out: &mut Vec<Row<'a>>) {
    for (i, child) in node.children().iter().enumerate() {
        path.push(i);
        out.push(Row {
            depth,
            path: path.clone(),
            node: child,
        });
        if !matches!(child, Node::Container(c) if c.collapsed) {
            walk(child, path, depth + 1, out);
        }
        path.pop();
    }
}

/// Set `collapsed` on every container below `node` (not `node` itself).
fn set_collapsed_all(node: &mut Node, collapsed: bool) {
    for child in node.children_mut().into_iter().flatten() {
        if let Node::Container(c) = child {
            c.collapsed = collapsed;
        }
        set_collapsed_all(child, collapsed);
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use chrono::Utc;

    use super::*;
    use crate::model::{
        container::{Container, ContainerKind},
        task::Task,
    };

    fn task(name: &str) -> Node {
        Node::Task(Task::new(name.into(), None, Utc::now()))
    }
    fn container(name: &str, children: Vec<Node>) -> Node {
        let mut c = Container::new(
            name.into(),
            PathBuf::from("/tmp").join(name),
            ContainerKind::Workspace,
        );
        c.children = children;
        Node::Container(c)
    }
    fn tree_with(children: Vec<Node>, cursor: &[usize]) -> Tree {
        Tree {
            root: container("root", children),
            cursor: cursor.to_vec(),
        }
    }

    /// root
    /// ├─ a                [0]
    /// ├─ inner            [1]
    /// │  ├─ b             [1,0]
    /// │  └─ deep          [1,1]
    /// │     └─ c          [1,1,0]
    /// ├─ z                [2]
    /// └─ empty (no kids)  [3]
    fn tree(cursor: &[usize]) -> Tree {
        tree_with(
            vec![
                task("a"),
                container("inner", vec![task("b"), container("deep", vec![task("c")])]),
                task("z"),
                container("empty", vec![]),
            ],
            cursor,
        )
    }

    fn all_paths() -> Vec<NodePath> {
        vec![
            vec![0],
            vec![1],
            vec![1, 0],
            vec![1, 1],
            vec![1, 1, 0],
            vec![2],
            vec![3],
        ]
    }

    // ---------- rows ----------

    #[test]
    fn rows_are_depth_first() {
        let t = tree(&[]);
        let names: Vec<_> = t.rows().iter().map(|r| r.node.name()).collect();
        assert_eq!(names, vec!["a", "inner", "b", "deep", "c", "z", "empty"]);
    }

    #[test]
    fn rows_have_depths() {
        let t = tree(&[]);
        let depths: Vec<_> = t.rows().iter().map(|r| r.depth).collect();
        assert_eq!(depths, vec![0, 0, 1, 1, 2, 0, 0]);
    }

    #[test]
    fn rows_have_paths_matching_get() {
        let t = tree(&[]);
        let rows = t.rows();
        let paths: Vec<_> = rows.iter().map(|r| r.path.clone()).collect();
        assert_eq!(paths, all_paths());
        for r in &rows {
            assert_eq!(t.get(&r.path).unwrap().name(), r.node.name());
        }
    }

    #[test]
    fn rows_empty_root() {
        assert!(tree_with(vec![], &[]).rows().is_empty());
    }

    // ---------- move_down / move_up ----------

    #[test]
    fn move_down_walks_all_rows_then_stays() {
        let mut t = tree(&[]); // after load: not on a row
        for expected in all_paths() {
            t.move_down();
            assert_eq!(t.cursor, expected);
        }
        t.move_down();
        assert_eq!(t.cursor, vec![3]); // stays on last
    }

    #[test]
    fn move_up_walks_back_then_stays() {
        let mut t = tree(&[3]);
        for expected in all_paths().into_iter().rev().skip(1) {
            t.move_up();
            assert_eq!(t.cursor, expected);
        }
        t.move_up();
        assert_eq!(t.cursor, vec![0]); // stays on first
    }

    #[test]
    fn move_up_from_no_row_selects_first() {
        let mut t = tree(&[]);
        t.move_up();
        assert_eq!(t.cursor, vec![0]);
    }

    #[test]
    fn move_from_stale_cursor_selects_first() {
        let mut t = tree(&[9, 9]); // e.g. node deleted elsewhere
        t.move_down();
        assert_eq!(t.cursor, vec![0]);
    }

    #[test]
    fn moves_on_empty_tree_keep_cursor_empty() {
        let mut t = tree_with(vec![], &[]);
        t.move_down();
        t.move_up();
        t.move_in();
        t.move_out();
        assert!(t.cursor.is_empty());
    }

    // ---------- move_in / move_out ----------

    #[test]
    fn move_in_enters_first_child() {
        let mut t = tree(&[1]);
        t.move_in();
        assert_eq!(t.cursor, vec![1, 0]);

        let mut t = tree(&[1, 1]);
        t.move_in();
        assert_eq!(t.cursor, vec![1, 1, 0]);
    }

    #[test]
    fn move_in_noop_on_task_and_empty_container() {
        let mut t = tree(&[0]); // task
        t.move_in();
        assert_eq!(t.cursor, vec![0]);

        let mut t = tree(&[3]); // container without children
        t.move_in();
        assert_eq!(t.cursor, vec![3]);
    }

    #[test]
    fn move_out_goes_to_parent() {
        let mut t = tree(&[1, 1, 0]);
        t.move_out();
        assert_eq!(t.cursor, vec![1, 1]);
        t.move_out();
        assert_eq!(t.cursor, vec![1]);
    }

    #[test]
    fn move_out_stops_at_top_level() {
        let mut t = tree(&[1]);
        t.move_out();
        assert_eq!(t.cursor, vec![1]); // never to the root itself

        let mut t = tree(&[]);
        t.move_out();
        assert!(t.cursor.is_empty());
    }

    // ---------- collapse / expand ----------

    fn names(t: &Tree) -> Vec<&str> {
        t.rows().iter().map(|r| r.node.name()).collect()
    }

    const ALL: [&str; 7] = ["a", "inner", "b", "deep", "c", "z", "empty"];

    #[test]
    fn collapse_hides_children() {
        let mut t = tree(&[1]);
        t.collapse();
        assert_eq!(names(&t), vec!["a", "inner", "z", "empty"]);
        assert_eq!(t.cursor, vec![1]);
    }

    #[test]
    fn collapse_nested_keeps_outer_open() {
        let mut t = tree(&[1, 1]);
        t.collapse();
        assert_eq!(names(&t), vec!["a", "inner", "b", "deep", "z", "empty"]);
    }

    #[test]
    fn collapse_on_task_collapses_parent_and_moves_cursor() {
        let mut t = tree(&[1, 0]); // task "b" inside "inner"
        t.collapse();
        assert_eq!(t.cursor, vec![1]);
        assert_eq!(names(&t), vec!["a", "inner", "z", "empty"]);
    }

    #[test]
    fn collapse_on_top_level_task_is_noop() {
        let mut t = tree(&[0]);
        t.collapse();
        assert_eq!(t.cursor, vec![0]);
        assert_eq!(names(&t), ALL);
    }

    #[test]
    fn expand_shows_children_again() {
        let mut t = tree(&[1]);
        t.collapse();
        t.expand();
        assert_eq!(names(&t), ALL);
    }

    #[test]
    fn inner_collapse_state_survives_outer_toggle() {
        let mut t = tree(&[1, 1]);
        t.collapse(); // deep
        t.move_out();
        t.collapse(); // inner
        t.expand(); // inner again
        assert_eq!(names(&t), vec!["a", "inner", "b", "deep", "z", "empty"]);
    }

    #[test]
    fn toggle_collapse_flips() {
        let mut t = tree(&[1]);
        t.toggle_collapse();
        assert_eq!(names(&t).len(), 4);
        t.toggle_collapse();
        assert_eq!(names(&t), ALL);
    }

    #[test]
    fn toggle_collapse_noop_on_task() {
        let mut t = tree(&[1, 0]);
        t.toggle_collapse();
        assert_eq!(t.cursor, vec![1, 0]);
        assert_eq!(names(&t), ALL);
    }

    #[test]
    fn move_down_skips_collapsed_children() {
        let mut t = tree(&[1]);
        t.collapse();
        t.move_down();
        assert_eq!(t.cursor, vec![2]); // "z", not "b"
    }

    #[test]
    fn move_in_expands_collapsed_container() {
        let mut t = tree(&[1]);
        t.collapse();
        t.move_in();
        assert_eq!(t.cursor, vec![1, 0]);
        assert_eq!(names(&t), ALL);
    }

    #[test]
    fn collapse_all_then_expand_all() {
        let mut t = tree(&[1, 1, 0]);
        t.collapse_all();
        assert_eq!(t.cursor, vec![1]); // top-level ancestor stays visible
        assert_eq!(names(&t), vec!["a", "inner", "z", "empty"]);

        t.expand_all();
        assert_eq!(t.cursor, vec![1]);
        assert_eq!(names(&t), ALL);
    }

    #[test]
    fn collapse_ops_on_empty_tree_are_noops() {
        let mut t = tree_with(vec![], &[]);
        t.collapse();
        t.expand();
        t.toggle_collapse();
        t.collapse_all();
        t.expand_all();
        assert!(t.cursor.is_empty());
        assert!(t.rows().is_empty());
    }
}
