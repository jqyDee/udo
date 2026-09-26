use crate::model::{NodePath, node::Node};

mod cursor;
mod edit;
mod folders;
mod load;

pub use folders::DirOwner;

/// In-memory tree node. Not serialized directly - Persistence goes through DTOs.
pub struct Tree {
    pub root: Node,
    pub cursor: NodePath,
}

impl Tree {
    /// Tree with `root`, cursor on nothing (`[]`).
    pub fn new(root: Node) -> Self {
        Self {
            root,
            cursor: vec![],
        }
    }

    /// Get the node at a given path:
    ///
    /// Path is structured as the children ids from the root node.
    pub fn get(&self, path: &[usize]) -> Option<&Node> {
        let mut cur = &self.root;
        for &i in path {
            cur = cur.children().get(i)?;
        }
        Some(cur)
    }

    /// Get the node at a given path (mut).
    ///
    /// Path is structured as the children ids from the root node.
    pub fn get_mut(&mut self, path: &[usize]) -> Option<&mut Node> {
        let mut cur = &mut self.root;
        for &i in path {
            cur = cur.children_mut()?.get_mut(i)?;
        }
        Some(cur)
    }

    /// Path of the nearest ancestor (or self) that owns a file.
    /// Task -> its parent container; container -> itself; missing -> None.
    pub fn nearest_file_owner(&self, path: &[usize]) -> Option<NodePath> {
        match self.get(path)? {
            Node::Container(_) => Some(path.to_vec()),
            Node::Task(_) => Some(path[..path.len() - 1].to_vec()), // go one up
        }
    }

    /// Path of the direct child of `parent` called `name` (task or container).
    /// None if `parent` is missing, is a task, or has no such child.
    pub fn find_child(&self, parent: &[usize], name: &str) -> Option<NodePath> {
        let parent_node = self.get(parent)?;
        let found_idx = parent_node
            .children()
            .iter()
            .position(|n| n.name() == name)?;
        let mut path = parent.to_vec();
        path.push(found_idx);
        Some(path)
    }

    /// Follow a chain of names from the root, e.g. `["work", "proj-a"]`.
    /// Empty `names` is the root (`Some(vec![])`).
    pub fn resolve(&self, names: &[&str]) -> Option<NodePath> {
        let mut path = vec![];
        for name in names {
            path = self.find_child(&path, name)?;
        }
        Some(path)
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use chrono::Utc;

    use crate::{
        model::{
            container::ContainerKind,
            node::NodePatch,
            task::{Task, TaskPatch},
            tree::Tree,
        },
        test_util::{container, container_at, task, tree_with},
    };

    pub(super) fn tree() -> Tree {
        tree_with(vec![task("a"), container("inner", vec![task("b")])], &[])
    }

    /// root (tmp) -> [task "a", ws (tmp/ws) -> [task "b"]], both files saved.
    pub(super) async fn disk_tree(root_dir: &Path) -> Tree {
        let ws_dir = root_dir.join("ws");
        std::fs::create_dir(&ws_dir).unwrap();
        let t = Tree::new(container_at(
            "root",
            root_dir,
            ContainerKind::Root,
            vec![
                task("a"),
                container_at("ws", &ws_dir, ContainerKind::Workspace, vec![task("b")]),
            ],
        ));
        t.save(&[]).await.unwrap();
        t.save(&[1]).await.unwrap();
        t
    }

    pub(super) fn new_task(name: &str, dir: Option<PathBuf>) -> Task {
        Task::new(name.into(), dir, Utc::now())
    }

    // ---------- lookup (tree() = root: [a, inner: [b]]) ----------

    #[test]
    fn tree_get() {
        let tree = tree();
        assert!(tree.get(&[]).is_some());
        assert_eq!(tree.get(&[0]).unwrap().name(), "a");
        assert_eq!(tree.get(&[1]).unwrap().name(), "inner");
        assert_eq!(tree.get(&[1, 0]).unwrap().name(), "b");
        assert!(tree.get(&[9]).is_none());
    }

    #[test]
    fn get_mut() {
        let mut tree = tree();
        tree.get_mut(&[0])
            .unwrap()
            .update(NodePatch::Task(TaskPatch {
                name: Some("x".into()),
                ..Default::default()
            }))
            .unwrap();
        assert_eq!(tree.get(&[0]).unwrap().name(), "x");
    }

    #[test]
    fn nearest_parent_container_from_container() {
        let t = tree();
        assert_eq!(t.nearest_file_owner(&[1]), Some(vec![1]));
    }

    #[test]
    fn nearest_parent_container_from_task() {
        let t = tree();
        assert_eq!(t.nearest_file_owner(&[1, 0]), Some(vec![1]));
    }

    #[test]
    fn nearest_file_owner_none_for_missing() {
        assert_eq!(tree().nearest_file_owner(&[9]), None);
    }

    #[test]
    fn find_child_by_name() {
        let t = tree();
        assert_eq!(t.find_child(&[], "a"), Some(vec![0]));
        assert_eq!(t.find_child(&[], "inner"), Some(vec![1]));
        assert_eq!(t.find_child(&[1], "b"), Some(vec![1, 0]));
    }

    #[test]
    fn find_child_none_cases() {
        let t = tree();
        assert_eq!(t.find_child(&[], "nope"), None); // no such child
        assert_eq!(t.find_child(&[0], "x"), None); // parent is a task
        assert_eq!(t.find_child(&[9], "a"), None); // parent missing
        assert_eq!(t.find_child(&[], "b"), None); // only direct children
    }

    #[test]
    fn resolve_follows_names() {
        let t = tree();
        assert_eq!(t.resolve(&[]), Some(vec![]));
        assert_eq!(t.resolve(&["inner"]), Some(vec![1]));
        assert_eq!(t.resolve(&["inner", "b"]), Some(vec![1, 0]));
    }

    #[test]
    fn resolve_none_on_broken_chain() {
        let t = tree();
        assert_eq!(t.resolve(&["nope"]), None);
        assert_eq!(t.resolve(&["inner", "nope"]), None);
        assert_eq!(t.resolve(&["a", "x"]), None); // "a" is a task
    }
}
