use std::path::{Path, PathBuf};

use crate::{
    Res,
    model::{NodePath, container::ContainerKind, node::Node, tree::Tree},
    naming::folder_name,
};

/// Result of `Tree::dir_owner`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DirOwner {
    /// A loaded node (container or task) at this path.
    Node(NodePath),
    /// A child dir of the container at this path that could not be loaded.
    Unloaded(NodePath),
}

impl Tree {
    /// Default dir for a new child container: `<parent dir>/<name>`.
    /// None if `parent` is missing or a task.
    pub fn default_child_dir(&self, parent: &[usize], name: &str) -> Option<PathBuf> {
        match self.get(parent)? {
            Node::Container(c) => Some(c.dir.join(folder_name(name)?)),
            Node::Task(_) => None,
        }
    }

    /// Automatic dir for a new task: `<project dir>/<name>` if `parent` is a
    /// Project, None elsewhere (tasks in workspaces / root get no folder).
    pub fn auto_task_dir(&self, parent: &[usize], name: &str) -> Option<PathBuf> {
        match self.get(parent)? {
            Node::Container(c) if c.kind == ContainerKind::Project => {
                Some(c.dir.join(folder_name(name)?))
            }
            _ => None,
        }
    }

    /// Who already uses `dir`, anywhere in the tree. Containers, tasks with a
    /// folder, and `unloaded` child dirs (still registered) count.
    pub fn dir_owner(&self, dir: &Path) -> Option<DirOwner> {
        fn walk(node: &Node, path: &mut NodePath, dir: &Path) -> Option<DirOwner> {
            if node.dir() == Some(dir) {
                return Some(DirOwner::Node(path.clone()));
            }
            let Node::Container(c) = node else {
                return None;
            };
            if c.unloaded.iter().any(|u| u == dir) {
                return Some(DirOwner::Unloaded(path.clone()));
            }
            for (i, child) in c.children.iter().enumerate() {
                path.push(i);
                let found = walk(child, path, dir);
                path.pop();
                if found.is_some() {
                    return found;
                }
            }
            None
        }
        walk(&self.root, &mut vec![], dir)
    }

    /// Error if `dir` is already used by another node (see `dir_owner`).
    /// Called by `create_*` before creating anything on disk.
    pub(super) fn check_dir_free(&self, dir: &Path) -> Res<()> {
        let owner = match self.dir_owner(dir) {
            None => return Ok(()),
            Some(DirOwner::Node(path)) => {
                let name = self.get(&path).map_or("?", |n| n.name());
                format!("{name:?}")
            }
            Some(DirOwner::Unloaded(parent)) => {
                let name = self.get(&parent).map_or("?", |n| n.name());
                format!("an unloaded container in {name:?}")
            }
        };
        Err(format!("{} is already used by {owner}", dir.display()).into())
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::model::{container::ContainerKind, node::Node, tree::tests::tree};

    #[test]
    fn default_child_dir_is_parent_dir_plus_name() {
        let t = tree(); // root (/tmp/root): [a, inner (/tmp/inner): [b]]
        assert_eq!(
            t.default_child_dir(&[1], "new"),
            Some(PathBuf::from("/tmp/inner/new"))
        );
        assert_eq!(
            t.default_child_dir(&[], "new"),
            Some(PathBuf::from("/tmp/root/new"))
        );
        assert_eq!(t.default_child_dir(&[0], "new"), None); // task parent
        assert_eq!(t.default_child_dir(&[9], "new"), None); // missing
    }

    #[test]
    fn auto_task_dir_only_inside_projects() {
        let mut t = tree(); // "inner" is a Workspace
        assert_eq!(t.auto_task_dir(&[1], "c"), None);
        assert_eq!(t.auto_task_dir(&[], "c"), None); // root

        if let Some(Node::Container(c)) = t.get_mut(&[1]) {
            c.kind = ContainerKind::Project;
        }
        assert_eq!(
            t.auto_task_dir(&[1], "c"),
            Some(PathBuf::from("/tmp/inner/c"))
        );
        assert_eq!(t.auto_task_dir(&[0], "c"), None); // task parent
    }
}
