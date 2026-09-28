//! The command line: parse, run one command, print its result. Each
//! command lives in its own file with its arguments, its result type and
//! the function that runs it; this file only dispatches and prints.

mod add;
mod edit;
mod ls;
mod mark;
mod parse;
mod report;
mod resolve;
mod rm;
mod settings;
mod show;
mod timer;

use std::path::Path;

use chrono::Local;
use clap::{Parser, Subcommand};

use crate::{
    Res,
    core::Core,
    model::{container::ContainerKind, time, tree::system_trash},
    tui,
};
use add::AddCommand;
use report::emit;

#[derive(Parser)]
#[command(
    name = "udo",
    about = "udo\n---------------------\ntask, time and workflow manager.",
    version,
    after_help = "NODE: a path like \"uni/cs/lab 3\", any unique end of one (\"lab 3\"), \
                  \"/\" for the root, or nothing for the node of the current folder.\n\n\
                  environment:\n  UDO_ROOT=<dir>  use <dir> as data root instead of ~/.config/udo"
)]
pub struct Cli {
    /// Print results as JSON
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// List the tree from NODE down
    Ls(ls::LsArgs),
    /// Show one node: its fields and the time tracked on it
    Show(show::ShowArgs),
    /// Add a task, project or workspace
    #[command(subcommand)]
    Add(AddCommand),
    /// Change a node's name, description, due date or kind
    Edit(edit::EditArgs),
    /// Remove a node (its files stay, unless --with-folder)
    Rm(rm::RmArgs),
    /// Mark a task done (stops its timer)
    Done(mark::DoneArgs),
    /// Give a task a status: todo, in-progress, stale, done
    Mark(mark::MarkArgs),
    /// Show a container's settings, or set / unset them
    Settings(settings::SettingsArgs),
    /// Start timing a task (a running one is stopped first)
    Start(timer::StartArgs),
    /// Stop the running timer
    Stop,
    /// Show what is being timed
    Status(timer::StatusArgs),
}

impl Cli {
    /// Run the command (no command: the TUI). `cwd` is where NODE
    /// arguments without a path start.
    pub async fn execute(&self, core: &mut Core, cwd: &Path) -> Res<()> {
        let Some(command) = &self.command else {
            return tui::run(core).await;
        };
        let json = self.json;
        match command {
            Command::Ls(a) => emit(&ls::run(core, cwd, a)?, json),
            Command::Show(a) => emit(&show::run(core, cwd, time::now(), a).await?, json),
            Command::Edit(a) => {
                let now = Local::now().naive_local();
                emit(&edit::run(core, cwd, now, a).await?, json)
            }
            Command::Rm(a) => {
                let mut ask = rm::ask_on_terminal;
                let removed = rm::run(core, cwd, time::now(), a, system_trash, &mut ask).await?;
                emit(&removed, json)
            }
            Command::Done(a) => emit(&mark::done(core, cwd, time::now(), a).await?, json),
            Command::Mark(a) => emit(&mark::mark(core, cwd, time::now(), a).await?, json),
            Command::Settings(a) => match &a.action {
                None => emit(&settings::show(core, cwd, a.node.as_deref())?, json),
                Some(action) => emit(&settings::change(core, cwd, action).await?, json),
            },
            Command::Add(AddCommand::Task(a)) => {
                let now = Local::now().naive_local();
                emit(&add::task(core, cwd, now, a).await?, json)
            }
            Command::Add(AddCommand::Project(a)) => {
                emit(&add::container(core, cwd, a, ContainerKind::Project).await?, json)
            }
            Command::Add(AddCommand::Workspace(a)) => {
                emit(&add::container(core, cwd, a, ContainerKind::Workspace).await?, json)
            }
            Command::Start(a) => emit(&timer::start(core, cwd, time::now(), a).await?, json),
            Command::Stop => emit(&timer::stop(core, time::now()).await?, json),
            Command::Status(a) => emit(&timer::status(core, time::now(), a).await?, json),
        }
    }
}
