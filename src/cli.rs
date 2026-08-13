use std::path::PathBuf;

use clap::{Parser, Subcommand};

use crate::{
    config::AppConfig,
    task::{add_project, add_task, add_workspace},
};

#[derive(Parser)]
#[command(name = "udo", about = "University task and script manager", version)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Subcommand)]
pub enum Commands {
    List,
    AddWorkspace {
        #[arg(short, long, value_parser = collapse_whitespaces)]
        name: String,
        #[arg(short, long)]
        dir: PathBuf,
        #[arg(short, long)]
        archive_dir: Option<PathBuf>,
    },
    AddProject {
        #[arg(short, long, value_parser = collapse_whitespaces)]
        workspace: String,
        #[arg(short, long, value_parser = collapse_whitespaces)]
        project: String,
    },
    AddTask {
        #[arg(short, long, value_parser = collapse_whitespaces)]
        project: Option<String>,
        #[arg(short, long, value_parser = collapse_whitespaces)]
        workspace: Option<String>,
        #[arg(short, long, value_parser = collapse_whitespaces)]
        task: String,
        #[arg(short, long)]
        due: String,
        #[arg(long)]
        custom_dir: Option<PathBuf>,
        #[arg(short, long)]
        no_auto_create_folder: bool,
    },
    Run {
        #[arg(short, long, value_parser = collapse_whitespaces)]
        project: String,
        #[arg(short, long, value_parser = collapse_whitespaces)]
        task: String,
    },
}

impl Cli {
    pub async fn execute(&self, app_config: &mut AppConfig) {
        match &self.command {
            None => {
                todo!("TUI is not done yet!")
            }
            Some(cmd) => match cmd {
                Commands::List => {
                    todo!("List is not yet done!");
                }
                Commands::AddWorkspace {
                    name,
                    dir,
                    archive_dir,
                } => {
                    if let Err(e) = add_workspace(app_config, name, dir, archive_dir).await {
                        eprintln!("Error adding workspace: {}", e);
                        std::process::exit(1);
                    }
                }
                Commands::AddProject { workspace, project } => {
                    if let Err(e) = add_project(app_config, workspace, project).await {
                        eprintln!("Error adding project: {}", e);
                        std::process::exit(1);
                    }
                }
                Commands::AddTask {
                    project,
                    workspace,
                    task,
                    due,
                    custom_dir,
                    no_auto_create_folder,
                } => {
                    if let Err(e) = add_task(
                        app_config,
                        workspace,
                        project,
                        task,
                        custom_dir,
                        due,
                        no_auto_create_folder,
                    )
                    .await
                    {
                        eprintln!("Error adding task: {}", e);
                        std::process::exit(1);
                    };
                }
                Commands::Run { project, task } => {
                    todo!("Run is not yet done; project: {}, task: {}!", project, task);
                }
            },
        }
    }
}

fn collapse_whitespaces(input: &str) -> Result<String, String> {
    let slug = input.split_whitespace().collect::<Vec<_>>().join("_");
    if slug.is_empty() {
        return Err("Workspace name cannot be empty".into());
    }
    Ok(slug)
}
