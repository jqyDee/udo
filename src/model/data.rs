use serde::{Deserialize, Serialize};
use tokio::fs;

use crate::{
    Res, UDO_FILE_NAME,
    model::{
        container::{Container, ContainerKind, ContainerSettings},
        id::NodeId,
        task::Task,
    },
    persist::write_toml_atomic,
};
use std::path::{Path, PathBuf};

/// On-disk shape of one container's `.udo.toml`.
///
/// `tasks` stores the terminal children as rows; `children` stores only the
/// *dirs* of child containers (not their nested content — those live in their
/// own files). This is what keeps it one-file-per-container.
#[derive(Serialize, Deserialize)]
pub struct ContainerData {
    pub id: NodeId,
    pub name: String,
    pub kind: ContainerKind,
    #[serde(default)]
    pub tasks: Vec<Task>, // terminal children (task rows)
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

// DTO conversion for a container
impl From<&Container> for ContainerData {
    fn from(c: &Container) -> Self {
        ContainerData {
            id: c.id,
            name: c.name.clone(),
            kind: c.kind,
            tasks: c.task_children(),
            // loaded children + ones we couldn't load (must not be dropped)
            children: c
                .container_children_paths()
                .into_iter()
                .chain(c.unloaded.iter().cloned())
                .collect(),
            settings: c.settings.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::container::ContainerKind;

    #[tokio::test]
    async fn container_data_roundtrips() {
        let dir = tempfile::tempdir().unwrap();
        let data = ContainerData {
            id: NodeId::new(),
            name: "w".into(),
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
        assert_eq!(loaded.name, "w");
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
    async fn ids_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let mut data = ContainerData::from(&Container::new(
            "w".into(),
            dir.path().to_path_buf(),
            ContainerKind::Workspace,
        ));
        data.tasks = vec![Task::new("t".into(), None, chrono::Utc::now())];
        data.save(dir.path()).await.unwrap();

        let loaded = ContainerData::load(dir.path()).await.unwrap();

        assert_eq!(loaded.id, data.id);
        assert_eq!(loaded.tasks[0].id, data.tasks[0].id);
    }
}
