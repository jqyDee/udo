use std::{collections::HashMap, error::Error, path::PathBuf};

use directories::BaseDirs;
use serde::{Deserialize, Serialize};
use tokio::fs;

use crate::{models::Task, persist::write_toml_atomic};

#[derive(Debug, Deserialize, Serialize)]
pub struct AppConfig {
    pub workspaces: HashMap<String, PathBuf>,
    pub default_workspace: Option<(String, PathBuf)>,
    pub theme: String,
    #[serde(default)]
    pub tasks: Vec<Task>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            workspaces: HashMap::new(),
            default_workspace: None,
            theme: "dark".to_string(),
            tasks: vec![],
        }
    }
}

impl AppConfig {
    pub async fn load_or_create() -> Result<Self, Box<dyn Error>> {
        let (config_dir, config_file) = find_config_path()?;

        if config_file.exists() {
            let contents = fs::read_to_string(&config_file).await?;
            let config: Self = toml::from_str(&contents)?;
            return Ok(config);
        }

        println!("First run detected! Creating config at {:?}", config_file);

        fs::create_dir_all(config_dir).await?;

        let default_config = AppConfig::default();

        write_toml_atomic(&config_file, &default_config).await?;

        Ok(default_config)
    }

    pub async fn save(&self) -> Result<(), Box<dyn Error>> {
        let (_, config_file) = find_config_path()?;
        write_toml_atomic(&config_file, &self).await?;
        Ok(())
    }
}

fn find_config_path() -> Result<(PathBuf, PathBuf), Box<dyn Error>> {
    let base_dirs = BaseDirs::new().ok_or("Could not acquire Base dirs")?;

    let config_dir = base_dirs.home_dir().join(".config").join("udo");
    let config_file = config_dir.join("config.toml");

    Ok((config_dir, config_file))
}
