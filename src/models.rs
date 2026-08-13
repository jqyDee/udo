use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Deserialize, Serialize)]
pub struct WorkspaceData {
    pub archive_dir: Option<PathBuf>,
    pub active_projects: Vec<PathBuf>,
    #[serde(default)]
    pub tasks: Vec<Task>,
}

impl WorkspaceData {
    pub fn new(archive_dir: Option<PathBuf>) -> Self {
        Self {
            archive_dir: archive_dir,
            active_projects: vec![],
            tasks: vec![],
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ProjectData {
    pub project_name: String,
    pub default_script: Option<PathBuf>,
    pub tasks: Vec<Task>,
}

impl ProjectData {
    pub fn new(project_name: String) -> Self {
        Self {
            project_name: project_name,
            default_script: None,
            tasks: Vec::new(),
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Task {
    pub name: String,
    pub dir: Option<PathBuf>,
    pub status: TaskStatus,
    pub due_date: DateTime<Utc>,
}

impl Task {
    pub fn new(name: String, dir: Option<PathBuf>, due_date: DateTime<Utc>) -> Self {
        Self {
            name: name,
            dir: dir,
            status: TaskStatus::Pending,
            due_date: due_date,
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub enum TaskStatus {
    Pending,
    InProgress,
    Finished,
}
