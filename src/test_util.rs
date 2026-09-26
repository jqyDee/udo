//! Shared builders for unit tests. In-memory only: nothing here is saved, so
//! the dirs are never written unless a test calls `save`.

use std::path::{Path, PathBuf};

use chrono::Utc;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::model::{
    container::{Container, ContainerKind},
    node::Node,
    task::Task,
    tree::Tree,
};

/// Pending task without a dir, due now.
pub fn task(name: &str) -> Node {
    Node::Task(Task::new(name.into(), None, Utc::now()))
}

/// Workspace container at `/tmp/<name>` with the given children.
pub fn container(name: &str, children: Vec<Node>) -> Node {
    let dir = PathBuf::from("/tmp").join(name);
    container_at(name, &dir, ContainerKind::Workspace, children)
}

/// Container of `kind` at a real `dir` (for tests that save/load).
pub fn container_at(name: &str, dir: &Path, kind: ContainerKind, children: Vec<Node>) -> Node {
    let mut c = Container::new(name.into(), dir.to_path_buf(), kind);
    c.children = children;
    Node::Container(c)
}

/// Tree whose root (`/tmp/root`) has `children`, cursor at `cursor`.
pub fn tree_with(children: Vec<Node>, cursor: &[usize]) -> Tree {
    let mut t = Tree::new(container("root", children));
    t.cursor = cursor.to_vec();
    t
}

/// Key press without modifiers.
pub fn press(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}
