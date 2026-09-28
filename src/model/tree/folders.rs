use std::path::{Path, PathBuf};

use crate::{
    Res,
    model::{NodePath, node::Node, settings::TaskFolderSetting, tree::Tree},
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
        let c = self.get(parent)?.as_container()?;
        Some(c.dir.join(folder_name(name)?))
    }

    /// Automatic dir for a new task: `<parent dir>/<name>`, unless the
    /// `task_folders` setting says `none` for `parent`.
    pub fn auto_task_dir(&self, parent: &[usize], name: &str) -> Option<PathBuf> {
        let c = self.get(parent)?.as_container()?;
        if self.setting(parent, |s| s.task_folders)?.value == TaskFolderSetting::None {
            return None;
        }
        Some(c.dir.join(folder_name(name)?))
    }

    /// Who already uses `dir`, anywhere in the tree. Containers, tasks with a
    /// folder, and `unloaded` child dirs (still registered) count.
    pub fn dir_owner(&self, dir: &Path) -> Option<DirOwner> {
        fn walk(node: &Node, path: &mut NodePath, dir: &Path) -> Option<DirOwner> {
            if node.dir() == Some(dir) {
                return Some(DirOwner::Node(path.clone()));
            }
            let c = node.as_container()?;
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

    use crate::model::{
        container::ContainerKind,
        node::Node,
        settings::TaskFolderSetting,
        tree::{Tree, tests::tree},
    };

    /// Set `task_folders` on the container at `path`.
    fn set_folders(t: &mut Tree, path: &[usize], value: TaskFolderSetting) {
        let c = t.get_mut(path).and_then(Node::as_container_mut).unwrap();
        c.settings.task_folders = Some(value);
    }

    #[test]
    fn default_child_dir_is_parent_dir_plus_name() {
        let t = tree(); // root (/tmp/root): [a, inner (/tmp/inner): [b]]
        assert_eq!(t.default_child_dir(&[1], "new"), Some(PathBuf::from("/tmp/inner/new")));
        assert_eq!(t.default_child_dir(&[], "new"), Some(PathBuf::from("/tmp/root/new")));
        assert_eq!(t.default_child_dir(&[0], "new"), None); // task parent
        assert_eq!(t.default_child_dir(&[9], "new"), None); // missing
    }

    // tree() = root (/tmp/root): [a, inner (/tmp/inner, Workspace): [b]]

    #[test]
    fn auto_task_dir_follows_the_setting() {
        let mut t = tree();
        set_folders(&mut t, &[], TaskFolderSetting::Auto);
        assert_eq!(t.auto_task_dir(&[], "c"), Some(PathBuf::from("/tmp/root/c")));
        assert_eq!(
            t.auto_task_dir(&[1], "c"),
            Some(PathBuf::from("/tmp/inner/c")) // inherited from the root
        );

        set_folders(&mut t, &[1], TaskFolderSetting::None);
        assert_eq!(t.auto_task_dir(&[1], "c"), None);
        assert!(t.auto_task_dir(&[], "c").is_some()); // the root is unaffected
    }

    #[test]
    fn a_child_can_switch_folders_back_on() {
        let mut t = tree();
        set_folders(&mut t, &[], TaskFolderSetting::None);
        set_folders(&mut t, &[1], TaskFolderSetting::Auto);

        assert_eq!(t.auto_task_dir(&[], "c"), None);
        assert_eq!(t.auto_task_dir(&[1], "c"), Some(PathBuf::from("/tmp/inner/c")));
    }

    #[test]
    fn auto_task_dir_ignores_the_kind() {
        let mut t = tree();
        let inner = t.get_mut(&[1]).and_then(Node::as_container_mut).unwrap();
        inner.kind = ContainerKind::Project;
        set_folders(&mut t, &[1], TaskFolderSetting::None);
        assert_eq!(t.auto_task_dir(&[1], "c"), None); // a project, but "none"

        let inner = t.get_mut(&[1]).and_then(Node::as_container_mut).unwrap();
        inner.kind = ContainerKind::Workspace;
        set_folders(&mut t, &[1], TaskFolderSetting::Auto);
        assert!(t.auto_task_dir(&[1], "c").is_some()); // a workspace, but "auto"
    }

    #[test]
    fn auto_task_dir_none_without_a_container_or_a_name() {
        let mut t = tree();
        set_folders(&mut t, &[], TaskFolderSetting::Auto);
        assert_eq!(t.auto_task_dir(&[0], "c"), None); // task parent
        assert_eq!(t.auto_task_dir(&[9], "c"), None); // missing
        assert_eq!(t.auto_task_dir(&[1], "  "), None); // no folder name
    }
}
