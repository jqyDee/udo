//! Looking a setting up: the node's own container, then its ancestors, then
//! `ContainerSettings::builtin()`.

use crate::model::{NodePath, node::Node, settings::ContainerSettings, tree::Tree};

/// A setting's value plus where it came from (for the editor:
/// `1h30 (from uni)`).
#[derive(Debug, Clone, PartialEq)]
pub struct Resolved<T> {
    pub value: T,
    pub source: Source,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    /// Set on the container itself.
    Own,
    /// Set on an ancestor, at this path.
    Inherited(NodePath),
    /// Nobody set it: `ContainerSettings::builtin()`.
    Default,
}

impl Tree {
    /// Effective value of one setting for the node at `path` (task: its
    /// container). Nearest container that sets it wins, then the built-in
    /// default. None: set nowhere and no default.
    ///
    /// `get` picks the field: `tree.setting(&path, |s| s.archive_dir.clone())`.
    pub fn setting<T>(
        &self,
        path: &[usize],
        get: impl Fn(&ContainerSettings) -> Option<T>,
    ) -> Option<Resolved<T>> {
        let start = self.nearest_file_owner(path)?;

        for depth in (0..=start.len()).rev() {
            let layer = &start[..depth];
            let Some(c) = self.get(layer).and_then(Node::as_container) else {
                continue;
            };

            if let Some(value) = get(&c.settings) {
                let source = if layer == path {
                    Source::Own
                } else {
                    Source::Inherited(layer.to_vec())
                };
                return Some(Resolved { value, source });
            };
        }

        builtin_setting(get)
    }

    /// What the container at `path` would get if it didn't set the value
    /// itself: its parent's value (root: the built-in default). For the
    /// placeholders of the settings form. None: not a container, or set
    /// nowhere above and no default.
    pub fn inherited_setting<T>(
        &self,
        path: &[usize],
        get: impl Fn(&ContainerSettings) -> Option<T>,
    ) -> Option<Resolved<T>> {
        self.get(path).and_then(Node::as_container)?;
        let Some((_, parent)) = path.split_last() else {
            return builtin_setting(get); // root: nothing above it
        };
        let mut found = self.setting(parent, get)?;
        // the parent's own value is inherited from the child's point of view
        if found.source == Source::Own {
            found.source = Source::Inherited(parent.to_vec());
        }
        Some(found)
    }
}

/// The built-in default of one setting, if it has one.
fn builtin_setting<T>(get: impl Fn(&ContainerSettings) -> Option<T>) -> Option<Resolved<T>> {
    get(&ContainerSettings::builtin()).map(|value| Resolved {
        value,
        source: Source::Default,
    })
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::{
        UDO_FILE_NAME,
        model::{container::ContainerKind, settings::TaskFolderSetting},
        test_util::{container, container_at, task, tree_with},
    };

    /// root
    /// ├─ uni          [0]
    /// │  └─ cs        [0,0]
    /// │     └─ lab    [0,0,0]  (task)
    /// └─ work         [1]
    fn tree() -> Tree {
        tree_with(vec![
            container("uni", vec![container("cs", vec![task("lab")])]),
            container("work", vec![]),
        ])
    }

    /// Set `archive_dir` on the container at `path`.
    fn set_archive(t: &mut Tree, path: &[usize], dir: &str) {
        let c = t.get_mut(path).and_then(Node::as_container_mut).unwrap();
        c.settings.archive_dir = Some(PathBuf::from(dir));
    }

    fn archive(t: &Tree, path: &[usize]) -> Option<Resolved<PathBuf>> {
        t.setting(path, |s| s.archive_dir.clone())
    }

    fn resolved(dir: &str, source: Source) -> Option<Resolved<PathBuf>> {
        Some(Resolved {
            value: PathBuf::from(dir),
            source,
        })
    }

    // ---------- lookup ----------

    #[test]
    fn own_value_wins_over_the_parent() {
        let mut t = tree();
        set_archive(&mut t, &[0], "/uni");
        set_archive(&mut t, &[0, 0], "/cs");

        assert_eq!(archive(&t, &[0, 0]), resolved("/cs", Source::Own));
    }

    #[test]
    fn inherited_from_the_parent() {
        let mut t = tree();
        set_archive(&mut t, &[0], "/uni");

        assert_eq!(
            archive(&t, &[0, 0]),
            resolved("/uni", Source::Inherited(vec![0]))
        );
    }

    #[test]
    fn nearest_ancestor_wins() {
        let mut t = tree();
        set_archive(&mut t, &[], "/root");
        set_archive(&mut t, &[0], "/uni");

        assert_eq!(
            archive(&t, &[0, 0]),
            resolved("/uni", Source::Inherited(vec![0]))
        );
    }

    #[test]
    fn root_value_reaches_every_container() {
        let mut t = tree();
        set_archive(&mut t, &[], "/root");

        for path in [&[0][..], &[0, 0], &[1]] {
            assert_eq!(
                archive(&t, path),
                resolved("/root", Source::Inherited(vec![])),
                "{path:?}"
            );
        }
        assert_eq!(archive(&t, &[]), resolved("/root", Source::Own));
    }

    #[test]
    fn a_task_gets_its_containers_value() {
        let mut t = tree();
        set_archive(&mut t, &[0, 0], "/cs");

        // tasks have no settings: the value is always inherited
        assert_eq!(
            archive(&t, &[0, 0, 0]),
            resolved("/cs", Source::Inherited(vec![0, 0]))
        );
    }

    #[test]
    fn siblings_do_not_inherit_from_each_other() {
        let mut t = tree();
        set_archive(&mut t, &[0], "/uni");

        assert_eq!(archive(&t, &[1]), None); // "work" is next to "uni", not below
    }

    #[test]
    fn set_nowhere_and_no_default_is_none() {
        assert_eq!(archive(&tree(), &[0, 0]), None);
    }

    #[test]
    fn missing_path_is_none() {
        let mut t = tree();
        set_archive(&mut t, &[], "/root");

        assert_eq!(archive(&t, &[9]), None);
        assert_eq!(archive(&t, &[0, 9, 9]), None);
    }

    // ---------- inherited_setting ----------

    fn inherited_archive(t: &Tree, path: &[usize]) -> Option<Resolved<PathBuf>> {
        t.inherited_setting(path, |s| s.archive_dir.clone())
    }

    #[test]
    fn inherited_skips_the_own_value() {
        let mut t = tree();
        set_archive(&mut t, &[0], "/uni");
        set_archive(&mut t, &[0, 0], "/cs");

        // the parent's own value, seen from the child: inherited, not Own
        assert_eq!(
            inherited_archive(&t, &[0, 0]),
            resolved("/uni", Source::Inherited(vec![0]))
        );
    }

    #[test]
    fn inherited_comes_from_further_up() {
        let mut t = tree();
        set_archive(&mut t, &[], "/root");
        set_archive(&mut t, &[0, 0], "/cs");

        assert_eq!(
            inherited_archive(&t, &[0, 0]),
            resolved("/root", Source::Inherited(vec![]))
        );
    }

    #[test]
    fn inherited_on_the_root_is_the_builtin() {
        let mut t = tree();
        set_archive(&mut t, &[], "/root");
        let root = t.get_mut(&[]).and_then(Node::as_container_mut).unwrap();
        root.settings.task_folders = Some(TaskFolderSetting::Auto);

        // its own values don't count
        assert_eq!(
            t.inherited_setting(&[], |s| s.task_folders),
            Some(Resolved {
                value: TaskFolderSetting::None,
                source: Source::Default,
            })
        );
        assert_eq!(inherited_archive(&t, &[]), None); // archive has no default
    }

    #[test]
    fn inherited_of_a_task_or_a_missing_path_is_none() {
        let mut t = tree();
        set_archive(&mut t, &[], "/root");

        assert_eq!(inherited_archive(&t, &[0, 0, 0]), None); // task "lab"
        assert_eq!(inherited_archive(&t, &[9]), None);
    }

    // ---------- files ----------

    #[tokio::test]
    async fn files_hold_only_what_is_set_there() {
        let tmp = tempfile::tempdir().unwrap();
        let root_dir = tmp.path().join("udo");
        let uni_dir = tmp.path().join("uni");
        let mut t = Tree::load_from(&root_dir).await.unwrap();
        set_archive(&mut t, &[], "/arch");
        t.save(&[]).await.unwrap();
        let uni = container_at("uni", &uni_dir, ContainerKind::Workspace, vec![]);
        let uni = t.create(&[], uni).await.unwrap();

        // root: exactly the one setting that is set
        let root_file = std::fs::read_to_string(root_dir.join(UDO_FILE_NAME)).unwrap();
        assert!(
            root_file.contains("archive_dir = \"/arch\""),
            "got:\n{root_file}"
        );
        for unset in ["task_folders", "default_deadline", "[root]"] {
            assert!(!root_file.contains(unset), "{unset} in:\n{root_file}");
        }
        // uni inherits it, but its file doesn't repeat it
        assert!(archive(&t, &uni).is_some());
        let uni_file = std::fs::read_to_string(uni_dir.join(UDO_FILE_NAME)).unwrap();
        assert!(!uni_file.contains("archive_dir"), "got:\n{uni_file}");

        // after a reload it is still inherited from the root
        let loaded = Tree::load_from(&root_dir).await.unwrap();
        assert_eq!(
            archive(&loaded, &uni),
            resolved("/arch", Source::Inherited(vec![]))
        );
    }
}
