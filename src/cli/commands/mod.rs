//! Every command: one file each, with its arguments, its result type and
//! the function that runs it. A group of subcommands (`add`, `settings`,
//! `session`) is a folder: one file per subcommand, and a `mod.rs` with the
//! subcommand enum and its `run`.
//!
//! A new command: its file (or folder), a variant in `Command`, an arm in
//! `run`.

mod add;
mod done;
mod edit;
mod ls;
mod rm;
mod session;
mod settings;
mod show;
mod timer;
mod track;

use std::path::Path;

use chrono::Local;
use clap::Subcommand;

use crate::{
    Res,
    cli::{confirm::ask_on_terminal, report::emit},
    core::Core,
    model::{time, tree::system_trash},
};

#[derive(Subcommand)]
pub enum Command {
    /// List the tree from NODE down
    Ls(ls::LsArgs),
    /// Show one node: its fields and the time tracked on it
    Show(show::ShowArgs),
    /// Add a task, project or workspace
    #[command(subcommand)]
    Add(add::AddCommand),
    /// Change a node's name, description, due date or kind
    Edit(edit::EditArgs),
    /// Remove a node (its files stay, unless --with-folder)
    Rm(rm::RmArgs),
    /// Mark a task done (stops its timer), or reopen it with --undo
    Done(done::DoneArgs),
    /// Show a container's settings, or set / unset them
    Settings(settings::SettingsArgs),
    /// Start timing a task (a running one is stopped first)
    Start(timer::StartArgs),
    /// Stop the running timer
    Stop,
    /// Show what is being timed
    Status(timer::StatusArgs),
    /// List and correct recorded time
    #[command(subcommand)]
    Session(session::SessionCommand),
    /// The timer for programs: tmux hooks, editor wrappers (quiet)
    #[command(subcommand)]
    Track(track::TrackCommand),
}

/// Run `command` and print its result (`json`: as JSON). `cwd` is where
/// NODE arguments without a path start.
pub async fn run(command: &Command, core: &mut Core, cwd: &Path, json: bool) -> Res<()> {
    match command {
        Command::Ls(a) => emit(&ls::run(core, cwd, time::now(), a).await?, json),
        Command::Show(a) => emit(&show::run(core, cwd, time::now(), a).await?, json),
        Command::Add(c) => add::run(c, core, cwd, json).await,
        Command::Edit(a) => {
            let now = Local::now().naive_local();
            emit(&edit::run(core, cwd, now, a).await?, json)
        }
        Command::Rm(a) => {
            let mut ask = ask_on_terminal;
            let removed = rm::run(core, cwd, time::now(), a, system_trash, &mut ask).await?;
            emit(&removed, json)
        }
        Command::Done(a) => emit(&done::run(core, cwd, time::now(), a).await?, json),
        Command::Settings(a) => settings::run(a, core, cwd, json).await,
        Command::Start(a) => emit(&timer::start(core, cwd, time::now(), a).await?, json),
        Command::Stop => emit(&timer::stop(core, time::now()).await?, json),
        Command::Status(a) => emit(&timer::status(core, time::now(), a).await?, json),
        Command::Session(c) => session::run(c, core, cwd, json).await,
        Command::Track(c) => track::run(c, core, cwd, json).await,
    }
}
