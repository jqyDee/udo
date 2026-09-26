use std::{fmt, path::PathBuf};

use serde::{Deserialize, Serialize};

use crate::model::node::{Node, NodeBody};

/// Container body of a `Node` (id and name live on the node).
pub struct Container {
    pub dir: PathBuf,
    pub kind: ContainerKind,
    pub settings: ContainerSettings,
    pub children: Vec<Node>,
    /// Child dirs listed in the file but not loadable (e.g. drive unmounted).
    /// Not shown in the tree, but written back on save so they stay registered.
    pub unloaded: Vec<PathBuf>,
    /// View state: children hidden in `Tree::rows()`. Runtime only, not saved.
    pub collapsed: bool,
}

impl Container {
    /// Create a new container
    pub fn new(dir: PathBuf, kind: ContainerKind) -> Self {
        Self {
            dir,
            kind,
            settings: ContainerSettings::default(),
            unloaded: vec![],
            collapsed: false,
            children: vec![],
        }
    }

    /// Dirs of direct Container children (the `children` list in the DTO).
    pub fn container_children_paths(&self) -> Vec<PathBuf> {
        self.children
            .iter()
            .filter_map(|n| match &n.body {
                NodeBody::Container(c) => Some(c.dir.clone()),
                NodeBody::Task(_) => None,
            })
            .collect()
    }

    /// Update the containers information
    pub fn update(&mut self, patch: ContainerPatch) {
        if let Some(dir) = patch.dir {
            self.dir = dir
        }
        if let Some(kind) = patch.kind {
            self.kind = kind
        }
        if let Some(settings) = patch.settings {
            self.settings = settings
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ContainerKind {
    Root,
    Workspace,
    Project,
}

impl ContainerKind {
    /// Every kind. Index = `index()`.
    pub const ALL: [Self; 3] = [Self::Root, Self::Workspace, Self::Project];

    /// Kinds a user can create (the root exists once, made on first run).
    pub const CREATABLE: [Self; 2] = [Self::Workspace, Self::Project];

    /// Lowercase name for the UI; also what `Display` prints.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Root => "root",
            Self::Workspace => "workspace",
            Self::Project => "project",
        }
    }

    /// Position in `ALL`.
    pub const fn index(self) -> usize {
        match self {
            Self::Root => 0,
            Self::Workspace => 1,
            Self::Project => 2,
        }
    }
}

/// Lowercase name for the UI, e.g. "new workspace", "created project x".
/// `pad`, so width/alignment like `{:<12}` work.
impl fmt::Display for ContainerKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.pad(self.label())
    }
}

#[derive(Default, Clone, Serialize, Deserialize)]
pub struct ContainerSettings {
    pub archive_dir: Option<PathBuf>,
    pub default_script: Option<PathBuf>,
    pub theme: Option<String>,                        // Global only
    pub default_workspace: Option<(String, PathBuf)>, // Global only
}

#[derive(Default)]
pub struct ContainerPatch {
    pub dir: Option<PathBuf>,
    pub kind: Option<ContainerKind>,
    pub settings: Option<ContainerSettings>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::{container_at, task};

    #[test]
    fn kind_index_matches_all_and_display_is_label() {
        for (i, kind) in ContainerKind::ALL.into_iter().enumerate() {
            assert_eq!(kind.index(), i, "{kind:?}");
            assert_eq!(kind.to_string(), kind.label());
        }
        assert!(!ContainerKind::CREATABLE.contains(&ContainerKind::Root));
    }

    #[test]
    fn container_update_kind() {
        let mut c = Container::new("/tmp/x".into(), ContainerKind::Workspace);
        c.update(ContainerPatch {
            kind: Some(ContainerKind::Project),
            ..Default::default()
        });
        assert_eq!(c.kind, ContainerKind::Project);
        assert_eq!(c.dir, PathBuf::from("/tmp/x"));
    }

    #[test]
    fn container_container_children_paths() {
        let path = PathBuf::from("/tmp/c_inner");
        let mut c = Container::new("/tmp/x".into(), ContainerKind::Workspace);
        c.children = vec![
            container_at("c_inner", &path, ContainerKind::Workspace, vec![]),
            task("t_inner"),
        ];
        let child_paths = c.container_children_paths();
        assert_eq!(child_paths.len(), 1);
        assert_eq!(child_paths[0], path);
    }
}
