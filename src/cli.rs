use std::{
    io,
    path::{PathBuf, absolute},
};

use chrono::NaiveDateTime;
use clap::{Parser, Subcommand};
use crossterm::event;

use crate::{
    Res,
    model::{
        container::{ContainerKind, ContainerPatch, ContainerSettings},
        node::NodePatch,
        task::Task,
        tree::{NodePath, Tree},
    },
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
    CreateWorkspace {
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
    pub async fn execute(&self, tree: &mut Tree) -> Res<()> {
        match &self.command {
            None => {
                let mut should_exit = false;
                ratatui::run(|terminal| {
                    loop {
                        terminal.draw(|frame| frame.render_widget("Hello World!", frame.area()))?;
                        if event::read()?.is_key_press() {
                            should_exit = true;
                            break;
                        }
                    }
                    Ok::<(), io::Error>(())
                })?;
                if should_exit {
                    return Ok(());
                }
            }
            Some(cmd) => match cmd {
                Commands::List => {
                    todo!("List is not yet done!");
                }
                Commands::CreateWorkspace {
                    name,
                    dir,
                    archive_dir,
                } => {
                    // absolute: the parent file stores this path, cwd must not matter
                    let dir = absolute(dir)?;
                    let ws = tree
                        .create_container(&[], name.clone(), dir.clone(), ContainerKind::Workspace)
                        .await?;

                    if let Some(archive_dir) = archive_dir {
                        tree.update(
                            &ws,
                            NodePatch::Container(ContainerPatch {
                                settings: Some(ContainerSettings {
                                    archive_dir: Some(absolute(archive_dir)?),
                                    ..Default::default()
                                }),
                                ..Default::default()
                            }),
                        )?;
                        tree.save(&ws).await?;
                    }
                    println!("Created workspace {name:?} at {dir:?}");
                }
                Commands::AddProject { workspace, project } => {
                    let ws = tree
                        .resolve(&[workspace.as_str()])
                        .ok_or("workspace not found")?;
                    let dir = node_dir(tree, &ws)?.join(project);
                    tree.create_container(&ws, project.clone(), dir.clone(), ContainerKind::Project)
                        .await?;
                    println!("Created project {project:?} at {dir:?}");
                }
                Commands::AddTask {
                    project,
                    workspace,
                    task,
                    due,
                    custom_dir,
                    no_auto_create_folder,
                } => {
                    let date = NaiveDateTime::parse_from_str(due, "%Y-%m-%d %H:%M")?.and_utc();

                    let parent: NodePath = match (workspace, project) {
                        (None, None) => vec![], // root
                        (Some(w), None) => tree
                            .resolve(&[w.as_str()])
                            .ok_or("workspace not found")?,
                        (Some(w), Some(p)) => tree
                            .resolve(&[w.as_str(), p.as_str()])
                            .ok_or("project not found")?,
                        (None, Some(_)) => {
                            return Err("cannot specify a project without a workspace".into());
                        }
                    };

                    // same as before: auto task folder only inside projects
                    let dir = match custom_dir {
                        Some(d) => Some(absolute(d)?),
                        None if project.is_some() && !no_auto_create_folder => {
                            Some(node_dir(tree, &parent)?.join(task))
                        }
                        None => None,
                    };

                    tree.create_task(&parent, Task::new(task.clone(), dir, date))
                        .await?;
                    println!("Added task {task:?}");
                }
                Commands::Run { project, task } => {
                    todo!("Run is not yet done; project: {}, task: {}!", project, task);
                }
            },
        }
        Ok(())
    }
}

/// Owned dir of the node at `path` (errors if missing or a task without dir).
fn node_dir(tree: &Tree, path: &[usize]) -> Res<PathBuf> {
    let dir = tree
        .get(path)
        .and_then(|n| n.dir())
        .ok_or("node has no dir")?;
    Ok(dir.to_path_buf())
}

fn collapse_whitespaces(input: &str) -> Result<String, String> {
    let slug = input.split_whitespace().collect::<Vec<_>>().join("_");
    if slug.is_empty() {
        return Err("Workspace name cannot be empty".into());
    }
    Ok(slug)
}
