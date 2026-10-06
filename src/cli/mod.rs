//! The command line: parse, run one command, print its result. The
//! commands live in `commands` (one file each, groups as folders); the rest
//! of `cli` is what they share: NODE resolving (`resolve`), value parsers
//! (`parse`), printing (`report`) and asking (`confirm`).

mod commands;
mod confirm;
mod parse;
mod report;
mod resolve;

use std::path::Path;

use clap::Parser;

use crate::{Res, core::Core, tui};
use commands::Command;

#[derive(Parser)]
#[command(
    name = "udo",
    about = "udo\n---------------------\ntask, time and workflow manager.",
    version,
    after_help = "NODE: a path like \"uni/cs/lab 3\", any unique end of one (\"lab 3\"), \
                  \"/\" for the root, id:<uuid> for a node by its ID (scripts), \
                  or nothing for the node of the current folder.\n\n\
                  environment:\n  UDO_ROOT=<dir>  use <dir> as data root instead of ~/.config/udo"
)]
pub struct Cli {
    /// Print results as JSON
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    command: Option<Command>,
}

impl Cli {
    /// Run the command (no command: the TUI). `cwd` is where NODE
    /// arguments without a path start.
    /// Afterwards, also after an error, `Core`'s warnings go to stderr.
    pub async fn execute(&self, core: &mut Core, cwd: &Path) -> Res<()> {
        match &self.command {
            Some(command) => {
                let result = commands::run(command, core, cwd, self.json).await;
                print_warnings(core);
                result
            }
            None => tui::run(core).await,
        }
    }
}

/// What `Core` noted besides the action (`estimate not recorded: …`): on
/// stderr, so `--json` output stays clean; the exit code stays as it is.
/// Also before a command's own `process::exit`, which skips `execute`.
pub(crate) fn print_warnings(core: &Core) {
    for warning in core.take_warnings() {
        eprintln!("warning: {warning}");
    }
}
