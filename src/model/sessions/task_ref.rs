use std::path::PathBuf;

use crate::model::{id::NodeId, node::Node};

/// What a session remembers about its task; survives a task delete.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskRef {
    pub id: NodeId,
    pub name: String,
    pub description: String,
    pub container_dir: PathBuf,
    pub container_id: NodeId,
}

impl TaskRef {
    pub fn of(node: &Node, parent_node: &Node) -> Option<Self> {
        // make sure this is actually a task, otherwise return immediately
        node.as_task()?;
        let container = parent_node.as_container()?;
        Some(Self {
            id: node.id(),
            name: node.name().into(),
            description: node
                .header
                .description
                .as_deref()
                .unwrap_or_default()
                .into(),
            container_dir: container.dir.clone(),
            container_id: parent_node.id(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::{container, task};

    #[test]
    fn a_task_takes_its_own_fields_and_its_containers() {
        let lab = task("lab 3").with_description(Some("sheet 3".into()));
        let cs = container("cs", vec![]);

        let r = TaskRef::of(&lab, &cs).unwrap();

        assert_eq!(r.id, lab.id());
        assert_eq!(r.name, "lab 3");
        assert_eq!(r.description, "sheet 3");
        assert_eq!(r.container_id, cs.id());
        assert_eq!(r.container_dir, PathBuf::from("/tmp/cs"));
    }

    #[test]
    fn no_description_is_empty() {
        let r = TaskRef::of(&task("lab 3"), &container("cs", vec![])).unwrap();

        assert_eq!(r.description, "");
    }

    #[test]
    fn a_container_is_not_a_task() {
        let cs = container("cs", vec![]);

        assert_eq!(TaskRef::of(&cs, &cs), None);
    }

    #[test]
    fn a_task_is_not_a_parent() {
        assert_eq!(TaskRef::of(&task("lab 3"), &task("a")), None);
    }
}
