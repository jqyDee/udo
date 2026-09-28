use crate::model::id::NodeId;

/// What a session remembers about its task; survives a task delete.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskRef {
    pub id: NodeId,
    pub name: String,
    pub description: String,
    pub container_path: String,
}
