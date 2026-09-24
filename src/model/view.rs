//! View state (which containers are collapsed), saved separately from the data.
//!
//! Lives in `<root dir>/view.toml`, never in a container's `.udo.toml`, so
//! collapsing in the TUI doesn't touch files in the user's project folders.
//! Containers are keyed by `dir` (stable), not by index path (shifts).

use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use tokio::fs;

use crate::{
    Res,
    model::{node::Node, tree::Tree},
    persist::write_toml_atomic,
};

pub const VIEW_FILE_NAME: &str = "view.toml";

#[derive(Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ViewState {
    /// Dirs of collapsed containers.
    #[serde(default)]
    pub collapsed: Vec<PathBuf>,
}

impl ViewState {
    /// Read `<root_dir>/view.toml`. Missing or broken file -> empty view:
    /// view state is disposable and must never stop the app from starting.
    pub async fn load(root_dir: &Path) -> Self {
        let path = root_dir.join(VIEW_FILE_NAME);
        let Ok(content) = fs::read_to_string(&path).await else {
            return Self::default();
        };
        toml::from_str(&content).unwrap_or_else(|e| {
            eprintln!("warning: ignoring broken {}: {e}", path.display());
            Self::default()
        })
    }

    /// Write `<root_dir>/view.toml` atomically.
    pub async fn save(&self, root_dir: &Path) -> Res<()> {
        write_toml_atomic(&root_dir.join(VIEW_FILE_NAME), self).await
    }
}

impl Tree {
    /// Set `collapsed` on every container from `view`. Containers not listed
    /// are expanded; listed dirs that no longer exist are ignored.
    pub fn apply_view(&mut self, view: &ViewState) {
        let collapsed: HashSet<&Path> = view.collapsed.iter().map(PathBuf::as_path).collect();
        apply(&mut self.root, &collapsed);
    }

    /// Current view: dirs of all collapsed containers, depth-first.
    pub fn view_state(&self) -> ViewState {
        let mut collapsed = vec![];
        collect(&self.root, &mut collapsed);
        ViewState { collapsed }
    }

    /// Save the current view next to the root's `.udo.toml`.
    pub async fn save_view(&self) -> Res<()> {
        let root_dir = self.root.dir().ok_or("root has no dir")?;
        self.view_state().save(root_dir).await
    }
}

fn apply(node: &mut Node, collapsed: &HashSet<&Path>) {
    if let Node::Container(c) = node {
        c.collapsed = collapsed.contains(c.dir.as_path());
        for child in &mut c.children {
            apply(child, collapsed);
        }
    }
}

fn collect(node: &Node, out: &mut Vec<PathBuf>) {
    if let Node::Container(c) = node {
        if c.collapsed {
            out.push(c.dir.clone());
        }
        for child in &c.children {
            collect(child, out);
        }
    }
}

#[cfg(test)]
mod tests {
    use chrono::Utc;

    use super::*;
    use crate::{
        UDO_FILE_NAME,
        model::{
            container::{Container, ContainerKind},
            task::Task,
        },
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
    /// root: [a, inner: [b, deep: [c]]]  (in memory, nothing written)
    fn tree() -> Tree {
        Tree {
            root: container(
                "root",
                vec![
                    task("a"),
                    container("inner", vec![task("b"), container("deep", vec![task("c")])]),
                ],
            ),
            cursor: vec![],
        }
    }
    fn names(t: &Tree) -> Vec<&str> {
        t.rows().iter().map(|r| r.node.name()).collect()
    }

    // ---------- Tree <-> ViewState ----------

    #[test]
    fn view_state_lists_collapsed_dirs() {
        let mut t = tree();
        t.cursor = vec![1, 1];
        t.collapse(); // deep
        t.move_out();
        t.collapse(); // inner

        let v = t.view_state();

        assert_eq!(
            v.collapsed,
            vec![PathBuf::from("/tmp/inner"), PathBuf::from("/tmp/deep")]
        );
    }

    #[test]
    fn view_state_empty_when_nothing_collapsed() {
        assert!(tree().view_state().collapsed.is_empty());
    }

    #[test]
    fn apply_view_collapses_listed_dirs() {
        let mut t = tree();
        t.apply_view(&ViewState {
            collapsed: vec!["/tmp/deep".into()],
        });
        assert_eq!(names(&t), vec!["a", "inner", "b", "deep"]);
    }

    #[test]
    fn apply_view_expands_unlisted() {
        let mut t = tree();
        t.collapse_all();
        t.apply_view(&ViewState::default());
        assert_eq!(names(&t), vec!["a", "inner", "b", "deep", "c"]);
    }

    #[test]
    fn apply_view_ignores_unknown_dirs() {
        let mut t = tree();
        t.apply_view(&ViewState {
            collapsed: vec!["/tmp/gone".into()],
        });
        assert_eq!(names(&t).len(), 5);
    }

    #[test]
    fn view_state_apply_roundtrip() {
        let mut t = tree();
        t.cursor = vec![1];
        t.collapse();
        let v = t.view_state();

        let mut fresh = tree();
        fresh.apply_view(&v);

        assert_eq!(names(&fresh), names(&t));
    }

    // ---------- file ----------

    #[tokio::test]
    async fn load_missing_file_is_empty() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(ViewState::load(dir.path()).await, ViewState::default());
    }

    #[tokio::test]
    async fn load_broken_file_is_empty() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(VIEW_FILE_NAME), "collapsed = 42").unwrap();
        assert_eq!(ViewState::load(dir.path()).await, ViewState::default());
    }

    #[tokio::test]
    async fn save_load_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let v = ViewState {
            collapsed: vec!["/tmp/a".into(), "/tmp/b".into()],
        };
        v.save(dir.path()).await.unwrap();
        assert_eq!(ViewState::load(dir.path()).await, v);
    }

    // ---------- end to end ----------

    #[tokio::test]
    async fn collapsed_state_survives_reload_without_touching_data_files() {
        let tmp = tempfile::tempdir().unwrap();
        let root_dir = tmp.path().join("root");
        let ws_dir = tmp.path().join("ws");

        let mut t = Tree::load_from(&root_dir).await.unwrap();
        let ws = t
            .create_container(&[], "ws".into(), ws_dir.clone(), ContainerKind::Workspace)
            .await
            .unwrap();
        t.create_task(&ws, Task::new("t".into(), None, Utc::now()))
            .await
            .unwrap();
        let ws_file_before = std::fs::read_to_string(ws_dir.join(UDO_FILE_NAME)).unwrap();

        t.cursor = ws;
        t.collapse();
        t.save_view().await.unwrap();

        let loaded = Tree::load_from(&root_dir).await.unwrap();
        assert_eq!(names(&loaded), vec!["ws"]); // "t" hidden
        let ws_file_after = std::fs::read_to_string(ws_dir.join(UDO_FILE_NAME)).unwrap();
        assert_eq!(ws_file_before, ws_file_after); // data file untouched
        assert!(root_dir.join(VIEW_FILE_NAME).exists());
    }
}
