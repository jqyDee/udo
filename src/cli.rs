use std::path::{PathBuf, absolute};

use chrono::{Local, NaiveDateTime, TimeZone, Utc};
use clap::{Parser, Subcommand};

use crate::{
    DATE_FMT, Res,
    model::{
        NodePath,
        container::{Container, ContainerKind},
        node::{Node, NodeBody},
        task::Task,
        tree::Tree,
    },
    naming::{folder_name, normalize_name},
    tui,
};

#[derive(Parser)]
#[command(
    name = "udo",
    about = "University task and script manager",
    version,
    after_help = "Environment:\n  UDO_ROOT=<dir>  use <dir> as data root instead of ~/.config/udo"
)]
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
        #[arg(long)]
        description: Option<String>,
    },
    AddProject {
        #[arg(short, long, value_parser = collapse_whitespaces)]
        workspace: String,
        #[arg(short, long, value_parser = collapse_whitespaces)]
        project: String,
        /// Project folder (relative to the cwd); default: `<workspace dir>/<project>`
        #[arg(short, long)]
        dir: Option<PathBuf>,
        #[arg(long)]
        description: Option<String>,
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
        #[arg(long)]
        description: Option<String>,
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
            None => tui::run(tree).await?,
            Some(cmd) => match cmd {
                Commands::List => print_tree(tree),
                Commands::CreateWorkspace {
                    name,
                    dir,
                    archive_dir,
                    description,
                } => {
                    // absolute: the parent file stores this path, cwd must not matter
                    let dir = absolute(dir)?;
                    let mut ws = Container::new(dir.clone(), ContainerKind::Workspace);
                    ws.settings.archive_dir = archive_dir.as_deref().map(absolute).transpose()?;

                    let node = Node::container(name.clone(), ws).with_description(description.clone());
                    tree.create(&[], node).await?;
                    println!("Created workspace {name:?} at {dir:?}");
                }
                Commands::AddProject {
                    workspace,
                    project,
                    dir,
                    description,
                } => {
                    let ws = tree
                        .resolve(&[workspace.as_str()])
                        .ok_or("workspace not found")?;
                    let dir = match dir {
                        // absolute: the parent file stores this path, cwd must not matter
                        Some(d) => absolute(d)?,
                        None => {
                            let folder = folder_name(project).ok_or("name cannot be empty")?;
                            node_dir(tree, &ws)?.join(folder)
                        }
                    };
                    let proj = Container::new(dir.clone(), ContainerKind::Project);
                    let node = Node::container(project.clone(), proj)
                        .with_description(description.clone());
                    tree.create(&ws, node).await?;
                    println!("Created project {project:?} at {dir:?}");
                }
                Commands::AddTask {
                    project,
                    workspace,
                    task,
                    due,
                    custom_dir,
                    no_auto_create_folder,
                    description,
                } => {
                    // typed as local time (like in the TUI), stored as UTC
                    let local = NaiveDateTime::parse_from_str(due, DATE_FMT)
                        .map_err(|_| format!("invalid --due {due:?}, use YYYY-MM-DD HH:MM"))?;

                    let date = Local
                        .from_local_datetime(&local)
                        .earliest()
                        .map(|t| t.with_timezone(&Utc))
                        .ok_or("that time doesn't exist (DST switch)")?;

                    let parent: NodePath = match (workspace, project) {
                        (None, None) => vec![], // root
                        (Some(w), None) => {
                            tree.resolve(&[w.as_str()]).ok_or("workspace not found")?
                        }
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
                        None if !no_auto_create_folder => tree.auto_task_dir(&parent, task),
                        None => None,
                    };

                    let node = Node::task(task.clone(), Task::new(dir, date))
                        .with_description(description.clone());
                    tree.create(&parent, node).await?;
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

/// `List`: one line per `tree.rows()` entry, indented by depth.
/// Containers: `name/` + kind. Tasks: name + status + due date.
fn print_tree(tree: &Tree) {
    let rows = tree.rows();
    if rows.is_empty() {
        println!("(nothing here yet)");
        return;
    }
    for row in rows {
        let indent = "  ".repeat(row.depth);
        let name = row.node.name();
        match &row.node.body {
            NodeBody::Container(c) => {
                println!("{:<30}{}", format!("{indent}{name}/"), c.kind);
            }
            NodeBody::Task(t) => {
                println!(
                    "{:<30}{:<12}{}",
                    format!("{indent}{name}"),
                    t.status,
                    t.due_date.with_timezone(&Local).format(DATE_FMT)
                );
            }
        }
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

/// Node names are stored like the TUI stores them (`normalize_name`), so
/// `-w "my uni"` finds a workspace made in the TUI.
fn collapse_whitespaces(input: &str) -> Result<String, String> {
    let name = normalize_name(input);
    if name.is_empty() {
        return Err("name cannot be empty".into());
    }
    Ok(name)
}
