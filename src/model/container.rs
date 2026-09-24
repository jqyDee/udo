use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::model::{node::Node, task::Task};

pub struct Container {
    pub name: String,
    pub dir: PathBuf,
    pub kind: ContainerKind,
    pub settings: ContainerSettings,
    pub children: Vec<Node>,
    /// Child dirs listed in the file but not loadable (e.g. drive unmounted).
    /// Not shown in the tree, but written back on save so they stay registered.
    pub unloaded: Vec<PathBuf>,
}

impl Container {
    /// Create a new container
    pub fn new(name: String, dir: PathBuf, kind: ContainerKind) -> Self {
        Self {
            name,
            dir,
            kind,
            settings: ContainerSettings::default(),
            unloaded: vec![],
            children: vec![],
        }
    }

    /// Direct Task children, cloned into a Vec (for the DTO).
    pub fn task_children(&self) -> Vec<Task> {
        self.children
            .iter()
            .filter_map(|n| match n {
                Node::Task(t) => Some(t.clone()),
                Node::Container(_) => None,
            })
            .collect()
    }

    /// Dirs of direct Container children (the `children` list in the DTO).
    pub fn container_children_paths(&self) -> Vec<PathBuf> {
        self.children
            .iter()
            .filter_map(|n| match n {
                Node::Container(c) => Some(c.dir.clone()),
                Node::Task(_) => None,
            })
            .collect()
    }

    /// Update the containers information
    pub fn update(&mut self, patch: ContainerPatch) {
        if let Some(name) = patch.name {
            self.name = name
        }
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

#[derive(Default, Clone, Serialize, Deserialize)]
pub struct ContainerSettings {
    pub archive_dir: Option<PathBuf>,
    pub default_script: Option<PathBuf>,
    pub theme: Option<String>,                        // Global only
    pub default_workspace: Option<(String, PathBuf)>, // Global only
}

#[derive(Default)]
pub struct ContainerPatch {
    pub name: Option<String>,
    pub dir: Option<PathBuf>,
    pub kind: Option<ContainerKind>,
    pub settings: Option<ContainerSettings>,
}

#[cfg(test)]
mod tests {
    use crate::model::task::TaskStatus;

    use super::*;
    use chrono::Utc;

    fn task(name: &str, dir: Option<PathBuf>) -> Task {
        Task {
            name: name.into(),
            dir,
            status: TaskStatus::Pending,
            due_date: Utc::now(),
        }
    }
    fn container(name: &str, children: Vec<Node>, path: Option<PathBuf>) -> Container {
        Container {
            name: name.into(),
            dir: path.unwrap_or_else(|| PathBuf::from("/tmp/x")),
            kind: ContainerKind::Workspace,
            settings: ContainerSettings::default(),
            unloaded: vec![],
            children,
        }
    }

    #[test]
    fn container_update_name() {
        let mut c = container("c", vec![], None);
        assert_eq!(c.name, "c");
        assert_eq!(c.kind, ContainerKind::Workspace);
        c.update(ContainerPatch {
            name: Some("new".into()),
            dir: None,
            kind: None,
            settings: None,
        });
        assert_eq!(c.name, "new");
        assert_eq!(c.kind, ContainerKind::Workspace);
    }

    #[test]
    fn container_task_children() {
        let t = task("t_inner", Some(PathBuf::from("/tmp/t_inner")));
        let c = container(
            "c",
            vec![
                Node::Container(container("c_inner", vec![], None)),
                Node::Task(t.clone()),
            ],
            None,
        );
        let tasks = c.task_children();
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0], t);
    }

    #[test]
    fn container_container_children_paths() {
        let path = PathBuf::from("/tmp/c_inner");
        let c = container(
            "c",
            vec![
                Node::Container(container("c_inner", vec![], Some(path.clone()))),
                Node::Task(task("t_inner", Some(PathBuf::from("/tmp/t_inner")))),
            ],
            None,
        );
        let child_paths = c.container_children_paths();
        assert_eq!(child_paths.len(), 1);
        assert_eq!(child_paths[0], path);
    }
}
