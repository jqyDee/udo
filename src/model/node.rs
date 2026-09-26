use std::path::Path;

use crate::{
    Res,
    model::{
        container::{Container, ContainerPatch},
        id::NodeId,
        task::{Task, TaskPatch},
    },
};

/// One entry of the tree. Fields every node has live here; the kind
/// specific part is the `body`.
pub struct Node {
    pub id: NodeId,
    pub name: String,
    pub body: NodeBody,
}

pub enum NodeBody {
    Container(Container),
    Task(Task),
}

impl Node {
    /// New task node with a fresh id.
    pub fn task(name: String, task: Task) -> Self {
        Self {
            id: NodeId::new(),
            name,
            body: NodeBody::Task(task),
        }
    }

    /// New container node with a fresh id.
    pub fn container(name: String, container: Container) -> Self {
        Self {
            id: NodeId::new(),
            name,
            body: NodeBody::Container(container),
        }
    }

    /// The container body, or None for a task.
    pub fn as_container(&self) -> Option<&Container> {
        match &self.body {
            NodeBody::Container(c) => Some(c),
            NodeBody::Task(_) => None,
        }
    }

    /// The container body (mut), or None for a task.
    pub fn as_container_mut(&mut self) -> Option<&mut Container> {
        match &mut self.body {
            NodeBody::Container(c) => Some(c),
            NodeBody::Task(_) => None,
        }
    }

    /// The task body, or None for a container.
    pub fn as_task(&self) -> Option<&Task> {
        match &self.body {
            NodeBody::Task(t) => Some(t),
            NodeBody::Container(_) => None,
        }
    }

    /// Children slice, or empty for a Task (leaf).
    pub fn children(&self) -> &[Node] {
        match &self.body {
            NodeBody::Container(c) => &c.children,
            NodeBody::Task(_) => &[],
        }
    }

    /// Mutable children, or None for a Task (leaf).
    pub fn children_mut(&mut self) -> Option<&mut Vec<Node>> {
        match &mut self.body {
            NodeBody::Container(c) => Some(&mut c.children),
            NodeBody::Task(_) => None,
        }
    }

    /// True if this node owns a file (every Container does; a Task does not).
    pub fn owns_file(&self) -> bool {
        match self.body {
            NodeBody::Container(_) => true,
            NodeBody::Task(_) => false,
        }
    }

    /// Apply a patch. Errors on kind mismatch, before anything is changed.
    pub fn update(&mut self, patch: NodePatch) -> Res<()> {
        match (&mut self.body, patch.body) {
            (NodeBody::Container(c), Some(BodyPatch::Container(p))) => c.update(p),
            (NodeBody::Task(t), Some(BodyPatch::Task(p))) => t.update(p),
            (_, None) => {}
            _ => return Err("patch kind does not match node kind".into()),
        }
        if let Some(name) = patch.name {
            self.name = name;
        }
        Ok(())
    }

    /// Filesystem dir this node lives at. Containers always have one; a
    /// Task may not (`Task.dir` is optional). Used by save/delete/TUI.
    pub fn dir(&self) -> Option<&Path> {
        match &self.body {
            NodeBody::Container(c) => Some(c.dir.as_path()),
            NodeBody::Task(t) => t.dir.as_deref(),
        }
    }

    /// Display name (container name or task name). Used by TUI rendering.
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn id(&self) -> NodeId {
        self.id
    }
}

/// Changes to a node. `None` fields stay as they are.
#[derive(Default)]
pub struct NodePatch {
    pub name: Option<String>,
    pub body: Option<BodyPatch>,
}

pub enum BodyPatch {
    Container(ContainerPatch),
    Task(TaskPatch),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::container::ContainerKind;
    use chrono::Utc;
    use std::path::PathBuf;

    fn task(name: &str, dir: Option<PathBuf>) -> Node {
        Node::task(name.into(), Task::new(dir, Utc::now()))
    }
    fn container(name: &str, children: Vec<Node>) -> Node {
        let mut c = Container::new("/tmp/x".into(), ContainerKind::Workspace);
        c.children = children;
        Node::container(name.into(), c)
    }

    #[test]
    fn container_has_children_task_has_none() {
        let n = container("ws", vec![task("a", None)]);
        assert_eq!(n.children().len(), 1);
        assert!(task("a", None).children().is_empty());
    }

    #[test]
    fn dir_none_when_task_has_no_dir() {
        assert!(task("a", None).dir().is_none());
        assert_eq!(
            task("a", Some("/tmp/t".into())).dir(),
            Some(Path::new("/tmp/t"))
        );
    }

    #[test]
    fn children_mut_can_push_on_container() {
        let mut n = container("ws", vec![]);
        n.children_mut().unwrap().push(task("a", None));
        assert_eq!(n.children().len(), 1);
    }

    #[test]
    fn update_renames_any_node() {
        let mut n = task("old", None);
        n.update(NodePatch {
            name: Some("new".into()),
            ..Default::default()
        })
        .unwrap();
        assert_eq!(n.name(), "new");
    }

    #[test]
    fn update_rejects_mismatched_patch_and_changes_nothing() {
        let mut n = task("t", None);
        let patch = NodePatch {
            name: Some("new".into()),
            body: Some(BodyPatch::Container(ContainerPatch::default())),
        };
        assert!(n.update(patch).is_err());
        assert_eq!(n.name(), "t");
    }
}
