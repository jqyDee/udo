//! Shared builders for unit tests. In memory unless the name says otherwise
//! (`disk_tree*`, `core`): nothing is saved, so the dirs are never written unless a
//! test calls `save`.

use std::path::{Path, PathBuf};

use chrono::{DateTime, FixedOffset, NaiveDate, NaiveDateTime, TimeZone};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use tempfile::TempDir;

use crate::{
    core::Core,
    model::{
        container::{Container, ContainerKind},
        id::NodeId,
        node::Node,
        sessions::{Owner, Session, SessionId, SessionSource, TaskRef},
        task::Task,
        time::{self, Time},
        tree::Tree,
    },
    storage::Storage,
    tui::{
        app::App,
        form::{FolderMode, Form, TaskDefaults},
        tree_state::TreeState,
    },
};

// ---------- times ----------

/// 2026-10-15 (a Thursday) at `h:m`, offset +02:00. Fixed, never
/// `time::now()`: results must not depend on when the test runs.
pub fn at(h: u32, m: u32) -> Time {
    FixedOffset::east_opt(2 * 3600)
        .unwrap()
        .with_ymd_and_hms(2026, 10, 15, h, m, 0)
        .unwrap()
}

/// 2026-10-15 at `h:m` in the local zone: for output shown in local time,
/// so the text is the same wherever the tests run.
pub fn local(h: u32, m: u32) -> Time {
    time::local_to_fixed(dt(2026, 10, 15, h, m)).unwrap()
}

/// Thursday 2026-10-15, 12:00 local.
pub fn thursday_noon() -> NaiveDateTime {
    dt(2026, 10, 15, 12, 0)
}

/// Local date and time, seconds 0.
pub fn dt(y: i32, mo: u32, d: u32, h: u32, mi: u32) -> NaiveDateTime {
    NaiveDate::from_ymd_opt(y, mo, d)
        .unwrap()
        .and_hms_opt(h, mi, 0)
        .unwrap()
}

/// `Time` from RFC 3339 text, e.g. `2026-10-15T09:00:00+02:00`.
pub fn parse_time(rfc3339: &str) -> Time {
    DateTime::parse_from_rfc3339(rfc3339).unwrap()
}

// ---------- sessions ----------

/// A manual session on task "lab 3" (in `/uni`) from `start` to `end`
/// (`None`: running), without a store: for code that only reads sessions.
pub fn session(start: Time, end: Option<Time>) -> Session {
    Session {
        id: SessionId::new(),
        task: TaskRef {
            id: NodeId::new(),
            name: "lab 3".into(),
            description: String::new(),
            container_dir: PathBuf::from("/uni"),
            container_id: NodeId::new(),
        },
        start,
        end,
        source: SessionSource::Manual,
        owner: Owner::manual(),
        created_at: start,
        edited_at: None,
        deleted_at: None,
    }
}

// ---------- nodes + trees ----------

/// Pending task without a dir, due now.
pub fn task(name: &str) -> Node {
    new_task(name, None)
}

/// Task node with an optional dir, due now (for `create`).
pub fn new_task(name: &str, dir: Option<PathBuf>) -> Node {
    Node::task(name.into(), Task::new(dir, time::now()))
}

/// Empty container node at `dir` (for `create`).
pub fn new_container(name: &str, dir: &Path, kind: ContainerKind) -> Node {
    container_at(name, dir, kind, vec![])
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

/// root
/// ├─ a                [0]
/// ├─ inner            [1]
/// │  ├─ b             [1,0]
/// │  └─ deep          [1,1]
/// │     └─ c          [1,1,0]
/// ├─ z                [2]
/// └─ empty (no kids)  [3]
pub fn deep_tree() -> Tree {
    tree_with(vec![
        task("a"),
        container("inner", vec![task("b"), container("deep", vec![task("c")])]),
        task("z"),
        container("empty", vec![]),
    ])
}

/// root
/// ├─ uni          [0]
/// │  └─ cs        [0,0]
/// │     └─ lab    [0,0,0]  (task)
/// └─ work         [1]
pub fn uni_tree() -> Tree {
    tree_with(vec![
        container("uni", vec![container("cs", vec![task("lab")])]),
        container("work", vec![]),
    ])
}

/// root (tmp) -> [task "a", ws (tmp/ws) -> [task "b"]], both files saved:
/// for tests whose operations write `.udo.toml`. Keep the `TempDir` alive.
pub async fn disk_tree() -> (TempDir, Tree) {
    let tmp = tempfile::tempdir().unwrap();
    let tree = disk_tree_in(tmp.path()).await;
    (tmp, tree)
}

/// `disk_tree` in an existing `root_dir`.
pub async fn disk_tree_in(root_dir: &Path) -> Tree {
    let ws_dir = root_dir.join("ws");
    std::fs::create_dir(&ws_dir).unwrap();
    let tree = Tree::new(container_at(
        "root",
        root_dir,
        ContainerKind::Root,
        vec![
            task("a"),
            container_at("ws", &ws_dir, ContainerKind::Workspace, vec![task("b")]),
        ],
    ));
    tree.save(&[]).await.unwrap();
    tree.save(&[1]).await.unwrap();
    tree
}

/// `Core` on `disk_tree`, with in-memory storage. Keep the `TempDir` alive.
pub async fn core() -> (TempDir, Core) {
    let (tmp, tree) = disk_tree().await;
    (tmp, Core::new(tree, Storage::in_memory()))
}

// ---------- run configs ----------

/// An executable script `name` in `<root>/run` (the default `run_dir`):
/// `body` after a `sh` shebang.
pub fn run_script(root: &Path, name: &str, body: &str) -> PathBuf {
    use std::{fs, os::unix::fs::PermissionsExt};
    let dir = root.join("run");
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    path
}

/// A run script `name` that writes `$UDO_EVENT $UDO_NODE_NAME
/// $UDO_TASK_NAME` into the returned file (`<root>/<name>.out`).
pub fn recorder(root: &Path, name: &str) -> PathBuf {
    let out = root.join(format!("{name}.out"));
    let body = format!("echo \"$UDO_EVENT $UDO_NODE_NAME $UDO_TASK_NAME\" > '{}'", out.display());
    run_script(root, name, &body);
    out
}

/// Fake Trash: deletes for real (inside the tempdir only).
pub fn fake_trash(p: &Path) -> Result<(), String> {
    std::fs::remove_dir_all(p).map_err(|e| e.to_string())
}

// ---------- tui ----------

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

/// Task form in `/uni/cs101` with `mode`, name typed in.
pub fn task_form(mode: FolderMode, name: &str) -> Form {
    let defaults = TaskDefaults {
        due: dt(2026, 6, 15, 12, 0),
        folder: mode,
    };
    let mut form = Form::new_task(vec![0], "cs101", Some("/uni/cs101".into()), defaults);
    for c in name.chars() {
        form.handle_key(press(KeyCode::Char(c)));
    }
    form
}
