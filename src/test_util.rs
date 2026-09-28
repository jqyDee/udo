//! Shared builders for unit tests. In memory unless the name says otherwise
//! (`disk_tree`): nothing is saved, so the dirs are never written unless a
//! test calls `save`.

use std::path::{Path, PathBuf};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use tempfile::TempDir;

use crate::{
    core::Core,
    model::{
        container::{Container, ContainerKind},
        node::Node,
        task::Task,
        time,
        tree::Tree,
    },
    storage::Storage,
    tui::{app::App, tree_state::TreeState},
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

/// An `App` for tests on `tree`, with in-memory storage. Read the tree back
/// through `app.core.tree()`. Leaked: the `Core` must outlive the `App`
/// without a `let` in every test, and the test process ends right after.
pub fn test_app(tree: Tree, state: TreeState) -> App<'static> {
    let core = Box::leak(Box::new(Core::new(tree, Storage::in_memory())));
    App::new(core, state)
}

/// root (tmp) -> [task "a", ws (tmp/ws) -> [task "b"]], both files saved:
/// for tests whose operations write `.udo.toml`. Keep the `TempDir` alive.
pub async fn disk_tree() -> (TempDir, Tree) {
    let tmp = tempfile::tempdir().unwrap();
    let ws_dir = tmp.path().join("ws");
    std::fs::create_dir(&ws_dir).unwrap();
    let tree = Tree::new(container_at(
        "root",
        tmp.path(),
        ContainerKind::Root,
        vec![
            task("a"),
            container_at("ws", &ws_dir, ContainerKind::Workspace, vec![task("b")]),
        ],
    ));
    tree.save(&[]).await.unwrap();
    tree.save(&[1]).await.unwrap();
    (tmp, tree)
}
