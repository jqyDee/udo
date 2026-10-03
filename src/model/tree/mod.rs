use std::path::PathBuf;

use crate::model::{
    NodePath,
    id::NodeId,
    node::{Node, NodeBody},
    settings::RootSettings,
};

mod edit;
mod folders;
mod load;
mod purge;
mod rows;

pub use purge::{PurgePlan, PurgeReport, TrashFn, system_trash};
pub use rows::Row;

/// In-memory tree node. Not serialized directly - Persistence goes through DTOs.
pub struct Tree {
    pub root: Node,
}

impl Tree {
    pub fn new(root: Node) -> Self {
        Self { root }
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
        if self.get(path)?.owns_file() {
            Some(path.to_vec())
        } else {
            Some(path[..path.len() - 1].to_vec()) // task: go one up
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

    /// Root-only settings (theme, default workspace, ...).
    pub fn root_settings(&self) -> &RootSettings {
        &self
            .root
            .as_container()
            .expect("the root is a container")
            .root_settings
    }

    /// The run config library: `run_dir` if set, else `<root>/run`. The
    /// one place that default lives.
    pub fn run_dir(&self) -> PathBuf {
        match &self.root_settings().run_dir {
            Some(dir) => dir.clone(),
            None => self.root.dir().expect("the root has a folder").join("run"),
        }
    }

    /// Ids of the tasks at or below `path`: a task is just itself, a
    /// container every task below it (at any depth), in tree order. No such
    /// node: empty.
    pub fn task_ids_below(&self, path: &[usize]) -> Vec<NodeId> {
        let Some(node) = self.get(path) else {
            return vec![];
        };

        match &node.body {
            NodeBody::Task(_) => vec![node.id()],
            NodeBody::Container(_) => self
                .rows()
                .into_iter()
                .filter(|r| r.path.starts_with(path) && r.node.as_task().is_some())
                .map(|r| r.node.id())
                .collect(),
        }
    }

    /// Path of the node with `id`; the root is `Some(vec![])`. `None` if no
    /// node has it (e.g. deleted).
    pub fn path_of(&self, id: NodeId) -> Option<NodePath> {
        if self.root.id() == id {
            return Some(vec![]);
        }
        self.rows()
            .into_iter()
            .find(|r| r.node.id() == id)
            .map(|r| r.path)
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::{
        model::{id::NodeId, node::Node, tree::Tree},
        test_util::{container, deep_tree, task, tree_with},
    };

    pub(super) fn tree() -> Tree {
        tree_with(vec![task("a"), container("inner", vec![task("b")])])
    }

    // ---------- task_ids_below (deep_tree: root: [a, inner: [b, deep: [c]], z, empty]) ----------

    /// Names of the tasks `task_ids_below(path)` returns, in its order.
    fn tasks_below(t: &Tree, path: &[usize]) -> Vec<String> {
        let ids = t.task_ids_below(path);
        t.rows()
            .into_iter()
            .filter(|r| ids.contains(&r.node.id()))
            .map(|r| r.node.name().to_string())
            .collect()
    }

    #[test]
    fn task_ids_below_a_task_is_itself() {
        let t = deep_tree();

        assert_eq!(t.task_ids_below(&[1, 0]), vec![t.get(&[1, 0]).unwrap().id()]);
    }

    #[test]
    fn task_ids_below_a_container_go_all_the_way_down() {
        let t = deep_tree();

        assert_eq!(tasks_below(&t, &[1]), vec!["b", "c"]);
        assert_eq!(tasks_below(&t, &[]), vec!["a", "b", "c", "z"]); // the root
    }

    #[test]
    fn task_ids_below_an_empty_or_missing_node_is_empty() {
        let t = deep_tree();

        assert!(t.task_ids_below(&[3]).is_empty()); // "empty": no children
        assert!(t.task_ids_below(&[9]).is_empty());
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
        tree.get_mut(&[0]).unwrap().header.name = "x".into();
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

    #[test]
    fn path_of_finds_any_node() {
        let t = deep_tree();
        let id = |path: &[usize]| t.get(path).unwrap().id();

        assert_eq!(t.path_of(id(&[1, 1, 0])), Some(vec![1, 1, 0])); // c, two levels down
        assert_eq!(t.path_of(id(&[1])), Some(vec![1])); // a container
        assert_eq!(t.path_of(id(&[])), Some(vec![])); // the root
    }

    #[test]
    fn run_dir_defaults_to_run_in_the_root() {
        let t = tree();

        assert_eq!(t.run_dir(), t.root.dir().unwrap().join("run"));
    }

    #[test]
    fn run_dir_set_wins() {
        let mut t = tree();
        let root = t.get_mut(&[]).and_then(Node::as_container_mut).unwrap();
        root.root_settings.run_dir = Some(PathBuf::from("/dotfiles/udo-run"));

        assert_eq!(t.run_dir(), PathBuf::from("/dotfiles/udo-run"));
    }

    #[test]
    fn path_of_an_unknown_id_is_none() {
        assert_eq!(tree().path_of(NodeId::new()), None);
    }
}
