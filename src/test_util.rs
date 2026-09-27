//! Shared builders for unit tests. In-memory only: nothing here is saved, so
//! the dirs are never written unless a test calls `save`.

use std::path::{Path, PathBuf};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::{
    model::{
        container::{Container, ContainerKind},
        node::Node,
        task::Task,
        time,
        tree::Tree,
    },
    tui::tree_state::TreeState,
};

/// Pending task without a dir, due now.
pub fn task(name: &str) -> Node {
    Node::task(name.into(), Task::new(None, time::now()))
}

/// Workspace container at `/tmp/<name>` with the given children.
pub fn container(name: &str, children: Vec<Node>) -> Node {
    let dir = PathBuf::from("/tmp").join(name);
    container_at(name, &dir, ContainerKind::Workspace, children)
}

/// Container of `kind` at a real `dir` (for tests that save/load).
pub fn container_at(name: &str, dir: &Path, kind: ContainerKind, children: Vec<Node>) -> Node {
    let mut c = Container::new(dir.to_path_buf(), kind);
    c.children = children;
    Node::container(name.into(), c)
}

/// Tree whose root (`/tmp/root`) has `children`.
pub fn tree_with(children: Vec<Node>) -> Tree {
    Tree::new(container("root", children))
}

/// Tree pane state with the cursor on `cursor`, nothing folded.
pub fn state_at(cursor: &[usize]) -> TreeState {
    TreeState {
        cursor: cursor.to_vec(),
        ..Default::default()
    }
}

/// Key press without modifiers.
pub fn press(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}
