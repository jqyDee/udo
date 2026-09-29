//! `udo add task|project|workspace NODE`: the last part of NODE is the new
//! name, the rest names its parent (see `resolve_parent`).

mod container;
mod task;

use std::{fmt, path::Path, path::PathBuf};

use chrono::Local;
use serde::Serialize;

use crate::{
    Res,
    cli::report::{Report, emit},
    core::Core,
    model::container::ContainerKind,
};

#[derive(clap::Subcommand)]
pub enum AddCommand {
    /// Add a task, e.g. `udo add task "uni/cs/lab 4" --due "fri 22:00"`
    Task(task::AddTaskArgs),
    /// Add a project, e.g. `udo add project uni/cs`
    Project(container::AddContainerArgs),
    /// Add a workspace, e.g. `udo add workspace uni --dir ~/uni`
    Workspace(container::AddContainerArgs),
}

/// What `add` created.
#[derive(Serialize)]
pub struct Added {
    /// `task`, `project` or `workspace`.
    pub what: String,
    pub path: String,
    pub dir: Option<PathBuf>,
}

/// Run `command` and print what it added.
pub async fn run(command: &AddCommand, core: &mut Core, cwd: &Path, json: bool) -> Res<()> {
    let added = match command {
        AddCommand::Task(a) => task::run(core, cwd, Local::now().naive_local(), a).await?,
        AddCommand::Project(a) => container::run(core, cwd, a, ContainerKind::Project).await?,
        AddCommand::Workspace(a) => container::run(core, cwd, a, ContainerKind::Workspace).await?,
    };
    emit(&added, json)
}

impl fmt::Display for Added {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "added {} {}", self.what, self.path)?;
        if let Some(dir) = &self.dir {
            write!(f, "\nfolder: {}", dir.display())?;
        }
        Ok(())
    }
}

impl Report for Added {}
