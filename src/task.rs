use std::path::PathBuf;

use chrono::NaiveDateTime;
use std::error::Error;
use tokio::fs;

use crate::{
    config::AppConfig,
    models::{ProjectData, Task, WorkspaceData},
    persist::write_toml_atomic,
};

const UDO_FILE_NAME: &str = ".udo.toml";

pub async fn add_workspace(
    app_config: &mut AppConfig,
    workspace_name: &str,
    workspace_dir: &PathBuf,
    archive_dir: &Option<PathBuf>,
) -> Result<(), Box<dyn Error>> {
    let workspace_dir_found = check_workspace_exists(app_config, workspace_name).await?;

    if let Some(dir) = workspace_dir_found {
        println!("Workspace already exists");

        // TODO: make this some kind of update
        if dir != workspace_dir {
            println!(
                "Provided workspace dir '{:?}' is not equal to the already registered workspace dir '{:?}'; Needs fixing in the future",
                dir, workspace_dir
            );
        }

        let workspace_toml_path = dir.join(UDO_FILE_NAME);
        if !workspace_toml_path.exists() {
            println!("Workspace toml not found; being recreated");
            let content = WorkspaceData::new(archive_dir.clone());
            write_toml_atomic(&workspace_toml_path, &content).await?;
        }
        return Ok(());
    }

    fs::create_dir_all(workspace_dir).await?;

    let workspace_toml_path = workspace_dir.join(UDO_FILE_NAME);
    let content = WorkspaceData::new(archive_dir.clone());
    write_toml_atomic(&workspace_toml_path, &content).await?;

    app_config
        .workspaces
        .insert(workspace_name.to_string(), workspace_dir.into());
    app_config.save().await?;

    Ok(())
}

pub async fn add_project(
    app_config: &AppConfig,
    workspace_name: &str,
    project_name: &str,
) -> Result<(), Box<dyn Error>> {
    let workspace_dir = check_workspace_exists(app_config, workspace_name)
        .await?
        .ok_or("Workspace could not be found")?;

    let workspace_toml_path = workspace_dir.join(UDO_FILE_NAME);

    let mut workspace_content: WorkspaceData = load_workspace_data(workspace_dir).await?;

    let project_dir = workspace_dir.join(project_name);

    if workspace_content
        .active_projects
        .iter()
        .any(|c| c == &project_dir)
    {
        println!("Project already exists");
        return Ok(());
    }

    fs::create_dir_all(&project_dir).await?;

    let project_toml_path = project_dir.join(UDO_FILE_NAME);
    let project_content = ProjectData::new(project_name.to_string());
    write_toml_atomic(&project_toml_path, &project_content).await?;

    workspace_content.active_projects.push(project_dir.clone());
    write_toml_atomic(&workspace_toml_path, &workspace_content).await?;

    println!(
        "Successfully added project '{}' to '{}'",
        project_name, workspace_name
    );

    Ok(())
}

pub async fn add_task(
    app_config: &mut AppConfig,
    workspace_name: &Option<String>,
    project_name: &Option<String>,
    task_name: &str,
    custom_dir: &Option<PathBuf>,
    due_date_str: &str,
    no_auto_create_folder: &bool,
) -> Result<(), Box<dyn Error>> {
    // Common things
    let date = NaiveDateTime::parse_from_str(due_date_str, "%Y-%m-%d %H:%M")?.and_utc();
    let mut new_task = Task::new(task_name.to_string(), custom_dir.clone(), date);

    match (workspace_name, project_name) {
        // Global
        (None, None) => {
            if app_config.tasks.iter().any(|t| t.name == task_name) {
                println!("Global task with the same name already exists");
                return Ok(());
            }

            app_config.tasks.push(new_task);
            app_config.save().await?;

            println!("Successfully added global task '{}'", task_name);
        }
        // Not allowed
        (None, Some(_)) => {
            return Err("Cannot specify a project without a workspace".into());
        }
        // Workspace
        (Some(wsn), None) => {
            let ws_dir = check_workspace_exists(app_config, wsn)
                .await?
                .ok_or("Workspace could not be found")?;

            let ws_toml_path = ws_dir.join(UDO_FILE_NAME);
            let mut ws_data = load_workspace_data(ws_dir).await?;

            if ws_data.tasks.iter().any(|t| t.name == task_name) {
                println!("Task already exists in workspace '{}'", wsn);
                return Ok(());
            }

            ws_data.tasks.push(new_task);
            write_toml_atomic(&ws_toml_path, &ws_data).await?;

            println!(
                "Successfully added workspace-level task '{}' to workspace '{}'",
                task_name, wsn
            );
        }
        // Project
        (Some(wsn), Some(pn)) => {
            let ws_dir = check_workspace_exists(app_config, wsn)
                .await?
                .ok_or("Workspace could not be found")?;

            let project_dir = check_project_exists(ws_dir, pn).await?;
            let project_toml_path = project_dir.join(UDO_FILE_NAME);

            let mut project_data = load_project_data(&project_dir).await?;

            if project_data.tasks.iter().any(|t| t.name == task_name) {
                println!("Task already exists in project '{}'", pn);
                return Ok(());
            }

            // defaults to creation
            if !no_auto_create_folder {
                let task_folder_path = match custom_dir {
                    Some(dir) => dir,
                    None => &project_dir.join(task_name),
                };
                new_task.dir = Some(task_folder_path.clone());
                fs::create_dir_all(&task_folder_path).await?;
            }

            project_data.tasks.push(new_task);
            write_toml_atomic(&project_toml_path, &project_data).await?;

            println!(
                "Successfully added project-level task '{}' to '{}'!",
                task_name, pn
            );
        }
    }

    Ok(())
}

async fn check_workspace_exists<'a>(
    app_config: &'a AppConfig,
    workspace_name: &str,
) -> Result<Option<&'a PathBuf>, Box<dyn Error>> {
    let workspace_dir = app_config.workspaces.get(workspace_name);
    if let Some(dir) = workspace_dir {
        fs::create_dir_all(dir).await?;
    }
    Ok(workspace_dir)
}

async fn check_project_exists(
    workspace_dir: &PathBuf,
    project_name: &str,
) -> Result<PathBuf, Box<dyn Error>> {
    let workspace_data: WorkspaceData = load_workspace_data(workspace_dir).await?;
    let project_dir = workspace_dir.join(project_name);

    if !workspace_data
        .active_projects
        .iter()
        .any(|ac| ac == &project_dir)
    {
        return Err(
            "Project not registered at workspace. Please initialize the project first".into(),
        );
    }

    fs::create_dir_all(&project_dir).await?;
    Ok(project_dir)
}

async fn load_workspace_data(workspace_dir: &PathBuf) -> Result<WorkspaceData, Box<dyn Error>> {
    let workspace_toml_path = workspace_dir.join(UDO_FILE_NAME);
    if !workspace_toml_path.exists() {
        return Err(format!(
            "Workspace TOML not found at {:?}. Please initialize the workspace first",
            workspace_toml_path
        )
        .into());
    }
    let content = fs::read_to_string(workspace_toml_path).await?;
    let workspace_data: WorkspaceData = toml::from_str(&content)?;
    Ok(workspace_data)
}

async fn load_project_data(project_dir: &PathBuf) -> Result<ProjectData, Box<dyn Error>> {
    let project_toml_path = project_dir.join(UDO_FILE_NAME);
    if !project_toml_path.exists() {
        return Err(format!(
            "Project TOML not found at {:?}. Please initialize the project first",
            project_toml_path
        )
        .into());
    }
    let content = fs::read_to_string(project_toml_path).await?;
    let project_data: ProjectData = toml::from_str(&content)?;
    Ok(project_data)
}
