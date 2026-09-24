use std::path::Path;

use crate::{
    Res,
    model::container::{Container, ContainerPatch},
    model::task::{Task, TaskPatch},
};

pub enum Node {
    Container(Container),
    Task(Task),
}

impl Node {
    /// Children slice, or empty for a Task (leaf).
    pub fn children(&self) -> &[Node] {
        match self {
            Node::Container(c) => &c.children,
            Node::Task(_) => &[],
        }
    }

    /// Mutable children, or None for a Task (leaf).
    pub fn children_mut(&mut self) -> Option<&mut Vec<Node>> {
        match self {
            Node::Container(c) => Some(&mut c.children),
            Node::Task(_) => None,
        }
    }

    /// True if this node owns a file (every Container does; a Task does not).
    pub fn owns_file(&self) -> bool {
        match self {
            Node::Container(_) => true,
            Node::Task(_) => false,
        }
    }

    /// Apply a patch, dispatching by node kind. Errors on kind mismatch.
    pub fn update(&mut self, patch: NodePatch) -> Res<()> {
        match (self, patch) {
            (Node::Container(c), NodePatch::Container(p)) => {
                c.update(p);
                Ok(())
            }
            (Node::Task(t), NodePatch::Task(p)) => {
                t.update(p);
                Ok(())
            }
            _ => Err("patch kind does not match node kind".into()),
        }
    }

    /// Filesystem dir this node lives at. Containers always have one; a
    /// Task may not (`Task.dir` is optional). Used by save/delete/TUI.
    pub fn dir(&self) -> Option<&Path> {
        match self {
            Node::Container(c) => Some(c.dir.as_path()),
            Node::Task(t) => t.dir.as_deref(),
        }
    }

    /// Display name (container name or task name). Used by TUI rendering.
    pub fn name(&self) -> &str {
        match self {
            Node::Container(c) => &c.name,
            Node::Task(t) => &t.name,
        }
    }
}

pub enum NodePatch {
    Container(ContainerPatch),
    Task(TaskPatch),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        model::container::{ContainerKind, ContainerSettings},
        model::task::TaskStatus,
    };
    use chrono::Utc;
    use std::path::PathBuf;

    fn task(name: &str, dir: Option<PathBuf>) -> Task {
        Task {
            name: name.into(),
            dir,
            status: TaskStatus::Pending,
            due_date: Utc::now(),
        }
    }
    fn container(name: &str, children: Vec<Node>) -> Container {
        Container {
            name: name.into(),
            dir: PathBuf::from("/tmp/x"),
            kind: ContainerKind::Workspace,
            settings: ContainerSettings::default(),
            children,
        }
    }

    #[test]
    fn container_has_children_task_has_none() {
        let n = Node::Container(container("ws", vec![Node::Task(task("a", None))]));
        assert_eq!(n.children().len(), 1);
        assert!(Node::Task(task("a", None)).children().is_empty());
    }

    #[test]
    fn dir_none_when_task_has_no_dir() {
        assert!(Node::Task(task("a", None)).dir().is_none());
        assert_eq!(
            Node::Task(task("a", Some("/tmp/t".into()))).dir(),
            Some(Path::new("/tmp/t"))
        );
    }

    #[test]
    fn children_mut_can_push_on_container() {
        let mut n = Node::Container(container("ws", vec![]));
        n.children_mut().unwrap().push(Node::Task(task("a", None)));
        assert_eq!(n.children().len(), 1);
    }

    #[test]
    fn update_applies_matching_task_patch() {
        let mut n = Node::Task(task("old", None));
        n.update(NodePatch::Task(TaskPatch {
            name: Some("new".into()),
            ..Default::default()
        }))
        .unwrap();
        assert_eq!(n.name(), "new");
    }

    #[test]
    fn update_rejects_mismatched_patch() {
        let mut n = Node::Task(task("t", None));
        assert!(
            n.update(NodePatch::Container(ContainerPatch::default()))
                .is_err()
        );
    }
}
