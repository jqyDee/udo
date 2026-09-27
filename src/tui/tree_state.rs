//! State of the tree pane: cursor, folded containers, list scroll. TUI only,
//! the `Tree` knows nothing about it.
//!
//! Folded containers and the selected node are saved to
//! `<root dir>/view.toml` (`ViewFile`), never into a container's
//! `.udo.toml`, so using the TUI doesn't touch the user's folders. Both are
//! keyed by node ID: they survive renames, moves and index shifts.

use std::collections::HashSet;

use ratatui::widgets::ListState;
use serde::{Deserialize, Serialize};
use tokio::fs;

use crate::{
    Res,
    model::{
        NodePath,
        id::NodeId,
        node::Node,
        tree::{Row, Tree},
    },
    persist::write_toml_atomic,
};

pub const VIEW_FILE_NAME: &str = "view.toml";

#[derive(Debug, Default)]
pub struct TreeState {
    /// Path of the selected node; `[]` = nothing selected.
    pub cursor: NodePath,
    /// Selection + scroll offset of the list widget (kept across frames).
    pub list: ListState,
    /// IDs of folded containers.
    pub collapsed: HashSet<NodeId>,
}

/// On-disk shape of `view.toml`.
#[derive(Default, Serialize, Deserialize)]
struct ViewFile {
    #[serde(default)]
    collapsed: HashSet<NodeId>,
    /// Node the cursor was on when udo closed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    selected: Option<NodeId>,
}

impl TreeState {
    // ---------- file ----------

    /// Read `view.toml` next to the root's `.udo.toml`. Missing or broken
    /// file -> empty state: view state is disposable and must never stop the
    /// app from starting. The cursor goes back to the node it was on, if
    /// that node still exists (by ID, wherever it is now).
    pub async fn load(tree: &Tree) -> Self {
        let Some(root_dir) = tree.root.dir() else {
            return Self::default();
        };
        let path = root_dir.join(VIEW_FILE_NAME);
        let file: ViewFile = match fs::read_to_string(&path).await {
            Ok(content) => toml::from_str(&content).unwrap_or_else(|e| {
                eprintln!("warning: ignoring broken {}: {e}", path.display());
                ViewFile::default()
            }),
            Err(_) => ViewFile::default(),
        };

        let cursor = file
            .selected
            .and_then(|id| tree.rows().into_iter().find(|r| r.node.header.id == id))
            .map(|r| r.path)
            .unwrap_or_default();
        Self {
            cursor,
            collapsed: file.collapsed,
            ..Self::default()
        }
    }

    /// Write `view.toml` next to the root's `.udo.toml`, atomically. IDs of
    /// nodes no longer in `tree` are dropped first, so the file doesn't grow
    /// forever.
    pub async fn save(&mut self, tree: &Tree) -> Res<()> {
        let root_dir = tree.root.dir().ok_or("root has no dir")?;
        let alive: HashSet<NodeId> = tree.rows().iter().map(|r| r.node.header.id).collect();
        self.collapsed.retain(|id| alive.contains(id));
        let file = ViewFile {
            collapsed: self.collapsed.clone(),
            selected: self.selected(tree).map(|n| n.header.id),
        };
        write_toml_atomic(&root_dir.join(VIEW_FILE_NAME), &file).await
    }

    // ---------- reading ----------

    /// Visible rows: like `Tree::rows`, but not inside folded containers.
    pub fn rows<'t>(&self, tree: &'t Tree) -> Vec<Row<'t>> {
        tree.rows_where(|n| !self.is_collapsed(n))
    }

    pub fn is_collapsed(&self, node: &Node) -> bool {
        self.collapsed.contains(&node.header.id)
    }

    /// Node at the cursor. None on the root (= nothing selected).
    pub fn selected<'t>(&self, tree: &'t Tree) -> Option<&'t Node> {
        if self.cursor.is_empty() {
            return None;
        }
        tree.get(&self.cursor)
    }

    // ---------- moving ----------

    /// Select the next row. Stays on the last row.
    /// Cursor not on any row (e.g. `[]` after load) -> first row.
    pub fn move_down(&mut self, tree: &Tree) {
        self.step(tree, true);
    }

    /// Select the previous row. Stays on the first row.
    /// Cursor not on any row -> first row.
    pub fn move_up(&mut self, tree: &Tree) {
        self.step(tree, false);
    }

    /// Go to the first child, if the cursor is on a container with children.
    /// A folded container is unfolded first.
    pub fn move_in(&mut self, tree: &Tree) {
        if has_children(self.selected(tree)) {
            self.expand(tree);
            self.cursor.push(0);
        }
    }

    /// Go to the parent. Never above the root's children (depth 0).
    pub fn move_out(&mut self) {
        if self.cursor.len() > 1 {
            self.cursor.pop();
        }
    }

    /// Put the cursor on `path` and unfold every container above it, so the
    /// row is visible (e.g. right after creating a node).
    pub fn reveal(&mut self, tree: &Tree, path: NodePath) {
        for depth in 1..path.len() {
            if let Some(n) = tree.get(&path[..depth]) {
                self.collapsed.remove(&n.header.id);
            }
        }
        self.cursor = path;
    }

    /// One row down (`down`) or up, clamped to the ends.
    fn step(&mut self, tree: &Tree, down: bool) {
        let rows = self.rows(tree);
        if rows.is_empty() {
            return;
        }
        let next = match rows.iter().position(|r| r.path == self.cursor) {
            None => 0,
            Some(i) if down => (i + 1).min(rows.len() - 1),
            Some(i) => i.saturating_sub(1),
        };
        self.cursor = rows[next].path.clone();
    }

    // ---------- folding ----------

    /// Fold the container at the cursor. On a task or an empty container, the
    /// parent is folded instead and the cursor moves onto it, so the cursor
    /// is never hidden. No-op on top-level tasks.
    pub fn collapse(&mut self, tree: &Tree) {
        if self.cursor.is_empty() {
            return;
        }
        if !has_children(self.selected(tree)) {
            if self.cursor.len() == 1 {
                return; // parent is the root, which can't be folded
            }
            self.cursor.pop();
        }
        self.set_collapsed(tree, true);
    }

    /// Unfold the container at the cursor.
    pub fn expand(&mut self, tree: &Tree) {
        self.set_collapsed(tree, false);
    }

    /// Fold or unfold the container at the cursor. No-op on tasks.
    pub fn toggle_collapse(&mut self, tree: &Tree) {
        let folded = self.selected(tree).is_some_and(|n| self.is_collapsed(n));
        self.set_collapsed(tree, !folded);
    }

    /// Fold every container. The cursor moves to its top-level ancestor,
    /// the only rows still visible.
    pub fn collapse_all(&mut self, tree: &Tree) {
        self.collapsed = tree
            .rows()
            .iter()
            .filter(|r| r.node.as_container().is_some())
            .map(|r| r.node.header.id)
            .collect();
        self.cursor.truncate(1);
    }

    /// Unfold every container.
    pub fn expand_all(&mut self) {
        self.collapsed.clear();
    }

    /// Fold or unfold the node at the cursor, if it is a container.
    fn set_collapsed(&mut self, tree: &Tree, collapsed: bool) {
        let Some(id) = self
            .selected(tree)
            .filter(|n| n.as_container().is_some())
            .map(|n| n.header.id)
        else {
            return;
        };
        if collapsed {
            self.collapsed.insert(id);
        } else {
            self.collapsed.remove(&id);
        }
    }

    // ---------- after tree changes ----------

    /// Keep the cursor valid after the node at `path` was removed from `tree`.
    pub fn after_remove(&mut self, tree: &Tree, path: &[usize]) {
        let Some((&idx, parent)) = path.split_last() else {
            return; // the root is never removed
        };
        let depth = parent.len();
        // Only cursors below the parent are affected.
        if self.cursor.len() <= depth || !self.cursor.starts_with(parent) {
            return;
        }

        let c = self.cursor[depth];
        if c > idx {
            // later sibling (or inside it): shifted left by one
            self.cursor[depth] -= 1;
        } else if c == idx {
            // on the removed node or inside it: take the sibling that moved
            // into the gap, else the previous one, else the parent
            let remaining = tree.get(parent).map_or(0, |p| p.children().len());
            self.cursor.truncate(depth);
            if remaining > 0 {
                self.cursor.push(idx.min(remaining - 1));
            }
        }
        // c < idx: earlier sibling, unaffected
    }
}

fn has_children(node: Option<&Node>) -> bool {
    node.is_some_and(|n| !n.children().is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        model::container::ContainerKind,
        test_util::{container, container_at, state_at, task, tree_with},
    };

    /// root
    /// ├─ a                [0]
    /// ├─ inner            [1]
    /// │  ├─ b             [1,0]
    /// │  └─ deep          [1,1]
    /// │     └─ c          [1,1,0]
    /// ├─ z                [2]
    /// └─ empty (no kids)  [3]
    fn setup(cursor: &[usize]) -> (Tree, TreeState) {
        let t = tree_with(vec![
            task("a"),
            container("inner", vec![task("b"), container("deep", vec![task("c")])]),
            task("z"),
            container("empty", vec![]),
        ]);
        (t, state_at(cursor))
    }

    fn empty() -> (Tree, TreeState) {
        (tree_with(vec![]), TreeState::default())
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

    fn names<'t>(s: &TreeState, t: &'t Tree) -> Vec<&'t str> {
        s.rows(t).iter().map(|r| r.node.name()).collect()
    }

    const ALL: [&str; 7] = ["a", "inner", "b", "deep", "c", "z", "empty"];

    // ---------- move_down / move_up ----------

    #[test]
    fn move_down_walks_all_rows_then_stays() {
        let (t, mut s) = setup(&[]); // after load: not on a row
        for expected in all_paths() {
            s.move_down(&t);
            assert_eq!(s.cursor, expected);
        }
        s.move_down(&t);
        assert_eq!(s.cursor, vec![3]); // stays on last
    }

    #[test]
    fn move_up_walks_back_then_stays() {
        let (t, mut s) = setup(&[3]);
        for expected in all_paths().into_iter().rev().skip(1) {
            s.move_up(&t);
            assert_eq!(s.cursor, expected);
        }
        s.move_up(&t);
        assert_eq!(s.cursor, vec![0]); // stays on first
    }

    #[test]
    fn move_up_from_no_row_selects_first() {
        let (t, mut s) = setup(&[]);
        s.move_up(&t);
        assert_eq!(s.cursor, vec![0]);
    }

    #[test]
    fn move_from_stale_cursor_selects_first() {
        let (t, mut s) = setup(&[9, 9]); // e.g. node deleted elsewhere
        s.move_down(&t);
        assert_eq!(s.cursor, vec![0]);
    }

    #[test]
    fn moves_on_empty_tree_keep_cursor_empty() {
        let (t, mut s) = empty();
        s.move_down(&t);
        s.move_up(&t);
        s.move_in(&t);
        s.move_out();
        assert!(s.cursor.is_empty());
    }

    // ---------- move_in / move_out ----------

    #[test]
    fn move_in_enters_first_child() {
        let (t, mut s) = setup(&[1]);
        s.move_in(&t);
        assert_eq!(s.cursor, vec![1, 0]);

        let (t, mut s) = setup(&[1, 1]);
        s.move_in(&t);
        assert_eq!(s.cursor, vec![1, 1, 0]);
    }

    #[test]
    fn move_in_noop_on_task_and_empty_container() {
        let (t, mut s) = setup(&[0]); // task
        s.move_in(&t);
        assert_eq!(s.cursor, vec![0]);

        let (t, mut s) = setup(&[3]); // container without children
        s.move_in(&t);
        assert_eq!(s.cursor, vec![3]);
    }

    #[test]
    fn move_out_goes_to_parent() {
        let mut s = state_at(&[1, 1, 0]);
        s.move_out();
        assert_eq!(s.cursor, vec![1, 1]);
        s.move_out();
        assert_eq!(s.cursor, vec![1]);
    }

    #[test]
    fn move_out_stops_at_top_level() {
        let mut s = state_at(&[1]);
        s.move_out();
        assert_eq!(s.cursor, vec![1]); // never to the root itself

        let mut s = TreeState::default();
        s.move_out();
        assert!(s.cursor.is_empty());
    }

    // ---------- collapse / expand ----------

    #[test]
    fn collapse_hides_children() {
        let (t, mut s) = setup(&[1]);
        s.collapse(&t);
        assert_eq!(names(&s, &t), vec!["a", "inner", "z", "empty"]);
        assert_eq!(s.cursor, vec![1]);
    }

    #[test]
    fn collapse_nested_keeps_outer_open() {
        let (t, mut s) = setup(&[1, 1]);
        s.collapse(&t);
        assert_eq!(names(&s, &t), vec!["a", "inner", "b", "deep", "z", "empty"]);
    }

    #[test]
    fn collapse_on_task_collapses_parent_and_moves_cursor() {
        let (t, mut s) = setup(&[1, 0]); // task "b" inside "inner"
        s.collapse(&t);
        assert_eq!(s.cursor, vec![1]);
        assert_eq!(names(&s, &t), vec!["a", "inner", "z", "empty"]);
    }

    #[test]
    fn collapse_on_top_level_task_is_noop() {
        let (t, mut s) = setup(&[0]);
        s.collapse(&t);
        assert_eq!(s.cursor, vec![0]);
        assert_eq!(names(&s, &t), ALL);
    }

    #[test]
    fn expand_shows_children_again() {
        let (t, mut s) = setup(&[1]);
        s.collapse(&t);
        s.expand(&t);
        assert_eq!(names(&s, &t), ALL);
    }

    #[test]
    fn inner_collapse_state_survives_outer_toggle() {
        let (t, mut s) = setup(&[1, 1]);
        s.collapse(&t); // deep
        s.move_out();
        s.collapse(&t); // inner
        s.expand(&t); // inner again
        assert_eq!(names(&s, &t), vec!["a", "inner", "b", "deep", "z", "empty"]);
    }

    #[test]
    fn toggle_collapse_flips() {
        let (t, mut s) = setup(&[1]);
        s.toggle_collapse(&t);
        assert_eq!(names(&s, &t).len(), 4);
        s.toggle_collapse(&t);
        assert_eq!(names(&s, &t), ALL);
    }

    #[test]
    fn toggle_collapse_noop_on_task() {
        let (t, mut s) = setup(&[1, 0]);
        s.toggle_collapse(&t);
        assert_eq!(s.cursor, vec![1, 0]);
        assert_eq!(names(&s, &t), ALL);
    }

    #[test]
    fn move_down_skips_collapsed_children() {
        let (t, mut s) = setup(&[1]);
        s.collapse(&t);
        s.move_down(&t);
        assert_eq!(s.cursor, vec![2]); // "z", not "b"
    }

    #[test]
    fn move_in_expands_collapsed_container() {
        let (t, mut s) = setup(&[1]);
        s.collapse(&t);
        s.move_in(&t);
        assert_eq!(s.cursor, vec![1, 0]);
        assert_eq!(names(&s, &t), ALL);
    }

    #[test]
    fn collapse_all_then_expand_all() {
        let (t, mut s) = setup(&[1, 1, 0]);
        s.collapse_all(&t);
        assert_eq!(s.cursor, vec![1]); // top-level ancestor stays visible
        assert_eq!(names(&s, &t), vec!["a", "inner", "z", "empty"]);

        s.expand_all();
        assert_eq!(s.cursor, vec![1]);
        assert_eq!(names(&s, &t), ALL);
    }

    #[test]
    fn collapse_ops_on_empty_tree_are_noops() {
        let (t, mut s) = empty();
        s.collapse(&t);
        s.expand(&t);
        s.toggle_collapse(&t);
        s.collapse_all(&t);
        s.expand_all();
        assert!(s.cursor.is_empty());
        assert!(s.rows(&t).is_empty());
    }

    #[test]
    fn folding_survives_a_rename() {
        let (mut t, mut s) = setup(&[1]);
        s.collapse(&t);
        t.get_mut(&[1]).unwrap().header.name = "renamed".into();
        assert_eq!(names(&s, &t), vec!["a", "renamed", "z", "empty"]);
    }

    // ---------- reveal ----------

    #[test]
    fn reveal_unfolds_every_ancestor_and_selects() {
        let (t, mut s) = setup(&[0]);
        s.collapse_all(&t); // inner and deep folded
        assert_eq!(names(&s, &t), vec!["a", "inner", "z", "empty"]);

        s.reveal(&t, vec![1, 1, 0]);

        assert_eq!(s.cursor, vec![1, 1, 0]);
        assert_eq!(names(&s, &t), ALL); // "c" visible: grandparent unfolded too
    }

    #[test]
    fn reveal_leaves_unrelated_containers_folded() {
        let (t, mut s) = setup(&[0]);
        s.collapse_all(&t);

        s.reveal(&t, vec![3]); // "empty", top level: nothing above to unfold

        assert_eq!(s.cursor, vec![3]);
        assert_eq!(names(&s, &t), vec!["a", "inner", "z", "empty"]);
    }

    // ---------- after_remove ----------

    /// root: [a, inner: [b]]. Remove `path` like `delete` does, without
    /// saving, and return the fixed-up cursor.
    fn cursor_after_remove(cursor: &[usize], path: &[usize]) -> NodePath {
        let mut t = tree_with(vec![task("a"), container("inner", vec![task("b")])]);
        let mut s = state_at(cursor);
        let (&idx, parent) = path.split_last().unwrap();
        t.get_mut(parent)
            .unwrap()
            .children_mut()
            .unwrap()
            .remove(idx);
        s.after_remove(&t, path);
        s.cursor
    }

    #[test]
    fn cursor_on_later_sibling_shifts_left() {
        assert_eq!(cursor_after_remove(&[1], &[0]), vec![0]);
    }

    #[test]
    fn cursor_inside_later_sibling_shifts_left() {
        assert_eq!(cursor_after_remove(&[1, 0], &[0]), vec![0, 0]);
    }

    #[test]
    fn cursor_on_earlier_sibling_unchanged() {
        assert_eq!(cursor_after_remove(&[0], &[1]), vec![0]);
    }

    #[test]
    fn cursor_on_removed_takes_next_sibling() {
        assert_eq!(cursor_after_remove(&[0], &[0]), vec![0]); // now "inner"
    }

    #[test]
    fn cursor_on_removed_last_takes_previous_sibling() {
        assert_eq!(cursor_after_remove(&[1], &[1]), vec![0]); // now "a"
    }

    #[test]
    fn cursor_inside_removed_subtree_goes_to_sibling() {
        assert_eq!(cursor_after_remove(&[1, 0], &[1]), vec![0]);
    }

    #[test]
    fn cursor_on_removed_only_child_goes_to_parent() {
        assert_eq!(cursor_after_remove(&[1, 0], &[1, 0]), vec![1]);
    }

    #[test]
    fn cursor_on_root_unchanged() {
        assert_eq!(cursor_after_remove(&[], &[0]), Vec::<usize>::new());
    }

    // ---------- file ----------

    /// Root at `dir` with `children` (in memory; `save` writes `view.toml`
    /// only).
    fn root_at(dir: &std::path::Path, children: Vec<Node>) -> Tree {
        Tree::new(container_at("root", dir, ContainerKind::Root, children))
    }

    #[tokio::test]
    async fn load_missing_file_is_empty() {
        let dir = tempfile::tempdir().unwrap();
        let s = TreeState::load(&root_at(dir.path(), vec![task("a")])).await;
        assert!(s.collapsed.is_empty());
        assert!(s.cursor.is_empty());
    }

    #[tokio::test]
    async fn load_broken_file_is_empty() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(VIEW_FILE_NAME), "collapsed = 42").unwrap();
        let s = TreeState::load(&root_at(dir.path(), vec![task("a")])).await;
        assert!(s.collapsed.is_empty());
        assert!(s.cursor.is_empty());
    }

    #[tokio::test]
    async fn save_load_roundtrip_and_drops_removed_ids() {
        let tmp = tempfile::tempdir().unwrap();
        let t = root_at(tmp.path(), vec![container("ws", vec![task("x")])]);
        let mut s = state_at(&[0]);
        s.collapse(&t);
        s.collapsed.insert(NodeId::new()); // a node deleted meanwhile

        s.save(&t).await.unwrap();

        let loaded = TreeState::load(&t).await;
        let ws = t.get(&[0]).unwrap().header.id;
        assert_eq!(loaded.collapsed, HashSet::from([ws]));
    }

    #[tokio::test]
    async fn cursor_survives_a_restart() {
        let tmp = tempfile::tempdir().unwrap();
        let t = root_at(
            tmp.path(),
            vec![task("a"), container("ws", vec![task("b"), task("c")])],
        );
        let mut s = state_at(&[1, 1]); // "c"

        s.save(&t).await.unwrap();

        assert_eq!(TreeState::load(&t).await.cursor, vec![1, 1]);
    }

    #[tokio::test]
    async fn cursor_follows_the_node_when_indices_shift() {
        let tmp = tempfile::tempdir().unwrap();
        let mut t = root_at(tmp.path(), vec![task("a"), task("b")]);
        let mut s = state_at(&[1]); // "b"
        s.save(&t).await.unwrap();

        // e.g. `udo add-task` from the CLI: a new node lands before "b"
        let root = t.get_mut(&[]).and_then(Node::children_mut).unwrap();
        root.insert(0, task("new"));

        let loaded = TreeState::load(&t).await;
        assert_eq!(loaded.cursor, vec![2]);
        assert_eq!(loaded.selected(&t).unwrap().name(), "b");
    }

    #[tokio::test]
    async fn deleted_node_leaves_the_cursor_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let mut t = root_at(tmp.path(), vec![task("a"), task("b")]);
        let mut s = state_at(&[1]);
        s.save(&t).await.unwrap();

        t.get_mut(&[])
            .and_then(Node::children_mut)
            .unwrap()
            .remove(1);

        assert!(TreeState::load(&t).await.cursor.is_empty()); // App picks row 1
    }

    #[tokio::test]
    async fn nothing_selected_writes_no_line() {
        let tmp = tempfile::tempdir().unwrap();
        let t = root_at(tmp.path(), vec![task("a")]);

        TreeState::default().save(&t).await.unwrap();

        let text = std::fs::read_to_string(tmp.path().join(VIEW_FILE_NAME)).unwrap();
        assert!(!text.contains("selected"), "got:\n{text}");
    }
}
