use serde::{Deserialize, Serialize};
use tokio::fs;

use crate::{
    Res, UDO_FILE_NAME,
    model::{
        container::{ContainerKind, ContainerSettings},
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
    /// Read <dir>/.udo.toml.
    pub async fn load(dir: &Path) -> Res<Self> {
        let content = fs::read_to_string(dir.join(UDO_FILE_NAME)).await?;
        Ok(toml::from_str(&content)?)
    }

    /// Write <dir>/.udo.toml atomically.
    pub async fn save(&self, dir: &Path) -> Res<()> {
        write_toml_atomic(&dir.join(UDO_FILE_NAME), self).await
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
}
