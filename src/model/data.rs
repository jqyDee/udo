use serde::{Deserialize, Serialize};
use tokio::fs;

use crate::{
    Res, UDO_FILE_NAME,
    model::{
        container::{ContainerKind, ContainerSettings},
        node::{Node, NodeBody, NodeHeader},
        task::Task,
    },
    persist::write_toml_atomic,
};
use std::path::{Path, PathBuf};

/// One task row in the parent's `.udo.toml`. Flat on disk: header and
/// `Task` fields sit side by side.
#[derive(Serialize, Deserialize)]
pub struct TaskData {
    #[serde(flatten)]
    pub header: NodeHeader,
    #[serde(flatten)]
    pub task: Task,
}

impl TaskData {
    pub fn into_node(self) -> Node {
        Node {
            header: self.header,
            body: NodeBody::Task(self.task),
        }
    }
}

/// On-disk shape of one container's `.udo.toml`.
///
/// `tasks` stores the terminal children as rows; `children` stores only the
/// *dirs* of child containers (not their nested content — those live in their
/// own files). This is what keeps it one-file-per-container.
#[derive(Serialize, Deserialize)]
pub struct ContainerData {
    #[serde(flatten)]
    pub header: NodeHeader,
    pub kind: ContainerKind,
    #[serde(default)]
    pub tasks: Vec<TaskData>, // terminal children (task rows)
    #[serde(default)]
    pub children: Vec<PathBuf>, // child container dirs
    #[serde(flatten)]
    pub settings: ContainerSettings,
}

impl ContainerData {
    /// Read <dir>/.udo.toml. Errors name the file, so a broken one is findable.
    pub async fn load(dir: &Path) -> Res<Self> {
        let path = dir.join(UDO_FILE_NAME);
        let content = fs::read_to_string(&path)
            .await
            .map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(toml::from_str(&content).map_err(|e| format!("{}: {e}", path.display()))?)
    }

    /// Write <dir>/.udo.toml atomically.
    pub async fn save(&self, dir: &Path) -> Res<()> {
        write_toml_atomic(&dir.join(UDO_FILE_NAME), self).await
    }
}

// DTO conversion for a container node. Fails for a task node.
impl TryFrom<&Node> for ContainerData {
    type Error = &'static str;

    fn try_from(node: &Node) -> Result<Self, Self::Error> {
        let c = node.as_container().ok_or("file owner is not a container")?;
        Ok(ContainerData {
            header: node.header.clone(),
            kind: c.kind,
            tasks: c
                .children
                .iter()
                .filter_map(|n| match &n.body {
                    NodeBody::Task(t) => Some(TaskData {
                        header: n.header.clone(),
                        task: t.clone(),
                    }),
                    NodeBody::Container(_) => None,
                })
                .collect(),
            // loaded children + ones we couldn't load (must not be dropped)
            children: c
                .container_children_paths()
                .into_iter()
                .chain(c.unloaded.iter().cloned())
                .collect(),
            settings: c.settings.clone(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        model::container::ContainerKind,
        test_util::{container, task},
    };

    #[tokio::test]
    async fn container_data_roundtrips() {
        let dir = tempfile::tempdir().unwrap();
        let data = ContainerData {
            header: NodeHeader::new("w".into()),
            kind: ContainerKind::Workspace,
            tasks: vec![],
            children: vec![PathBuf::from("/tmp/sub")],
            settings: ContainerSettings {
                archive_dir: Some(PathBuf::from("/tmp/arch")),
                ..Default::default()
            },
        };
        data.save(dir.path()).await.unwrap();

        let loaded = ContainerData::load(dir.path()).await.unwrap();
        assert_eq!(loaded.header.name, "w");
        assert_eq!(loaded.children, vec![PathBuf::from("/tmp/sub")]);
        // #[serde(flatten)] settings must survive the round-trip
        assert_eq!(
            loaded.settings.archive_dir,
            Some(PathBuf::from("/tmp/arch"))
        );
    }

    #[tokio::test]
    async fn load_error_names_the_file() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(UDO_FILE_NAME), "nope = 1").unwrap();

        let err = ContainerData::load(dir.path())
            .await
            .err()
            .unwrap()
            .to_string();

        assert!(err.contains(&dir.path().join(UDO_FILE_NAME).display().to_string()));
    }

    #[tokio::test]
    async fn load_missing_file_names_the_file() {
        let dir = tempfile::tempdir().unwrap();

        let err = ContainerData::load(dir.path())
            .await
            .err()
            .unwrap()
            .to_string();

        assert!(err.contains(UDO_FILE_NAME));
    }

    #[tokio::test]
    async fn headers_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let mut node = container("w", vec![task("t")]);
        node.header.description = Some("line one\nline two".into());
        let data = ContainerData::try_from(&node).unwrap();
        data.save(dir.path()).await.unwrap();

        let loaded = ContainerData::load(dir.path()).await.unwrap();

        // whole header: id, name, created_at, description
        assert_eq!(loaded.header, node.header);
        assert_eq!(loaded.tasks[0].header, node.children()[0].header);
    }

    #[test]
    fn try_from_keeps_task_rows_only() {
        let node = container("w", vec![container("inner", vec![]), task("t")]);
        let data = ContainerData::try_from(&node).unwrap();
        assert_eq!(data.header.name, "w");
        let names: Vec<_> = data.tasks.iter().map(|t| t.header.name.as_str()).collect();
        assert_eq!(names, vec!["t"]);
        assert_eq!(data.children, vec![PathBuf::from("/tmp/inner")]);
    }

    #[test]
    fn try_from_rejects_task_node() {
        assert!(ContainerData::try_from(&task("t")).is_err());
    }

    #[test]
    fn file_is_flat_and_skips_empty_description() {
        let node = container("w", vec![task("t")]);
        let text = toml::to_string(&ContainerData::try_from(&node).unwrap()).unwrap();
        assert!(text.starts_with("id = "), "got:\n{text}"); // header fields at the top
        assert!(text.contains("created_at = "));
        assert!(text.contains("[[tasks]]"));
        // no nested tables from `flatten`
        assert!(!text.contains("[header]") && !text.contains("[tasks.header]"));
        assert!(!text.contains("[tasks.task]"));
        assert!(!text.contains("description")); // None -> no line
    }
}
