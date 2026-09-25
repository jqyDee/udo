//! Shared builders for unit tests. In-memory only: nothing here is saved, so
//! the `/tmp/<name>` dirs are never written unless a test calls `save`.

use std::path::PathBuf;

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
    let mut c = Container::new(
        name.into(),
        PathBuf::from("/tmp").join(name),
        ContainerKind::Workspace,
    );
    c.children = children;
    Node::Container(c)
}

/// Tree whose root (`/tmp/root`) has `children`, cursor at `cursor`.
pub fn tree_with(children: Vec<Node>, cursor: &[usize]) -> Tree {
    Tree {
        root: container("root", children),
        cursor: cursor.to_vec(),
    }
}

/// Key press without modifiers.
pub fn press(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}
