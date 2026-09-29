//! `udo settings [NODE]`, `udo settings set [NODE] KEY=VALUE…`,
//! `udo settings unset [NODE] KEY…`. Settings belong to containers; on a
//! task they are its container's (like the TUI's settings tab). Values are
//! parsed by the same `SETTINGS` / `ROOT_SETTINGS` table as the TUI form.

mod change;
mod show;

use std::path::Path;

use crate::{Res, cli::report::emit, core::Core, model::NodePath};

#[derive(clap::Args)]
#[command(args_conflicts_with_subcommands = true)]
pub struct SettingsArgs {
    #[command(subcommand)]
    pub action: Option<SettingsAction>,
    /// Default: the node of the current folder (a task: its container)
    pub node: Option<String>,
}

#[derive(clap::Subcommand)]
pub enum SettingsAction {
    /// Set values, e.g. `udo settings set uni estimate=1h30`; the node can
    /// be left out (the current folder)
    Set {
        /// [NODE] KEY=VALUE...
        #[arg(required = true, num_args = 1.., value_name = "KEY=VALUE")]
        args: Vec<String>,
    },
    /// Unset values, so they are inherited again, e.g. `udo settings unset
    /// uni estimate`; the node can be left out (the current folder)
    Unset {
        /// [NODE] KEY...
        #[arg(required = true, num_args = 1.., value_name = "KEY")]
        args: Vec<String>,
    },
}

/// Show the settings (no action) or change them, and print the result.
pub async fn run(args: &SettingsArgs, core: &mut Core, cwd: &Path, json: bool) -> Res<()> {
    match &args.action {
        None => emit(&show::run(core, cwd, args.node.as_deref())?, json),
        Some(action) => emit(&change::run(core, cwd, action).await?, json),
    }
}

/// The container a node's settings live in: itself, or a task's container.
fn container_of(core: &Core, path: NodePath) -> Res<NodePath> {
    Ok(core
        .tree()
        .nearest_file_owner(&path)
        .ok_or("no such node")?)
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use crate::cli::Cli;

    #[test]
    fn show_and_set_parse() {
        assert!(Cli::try_parse_from(["udo", "settings"]).is_ok());
        assert!(Cli::try_parse_from(["udo", "settings", "uni"]).is_ok());
        assert!(Cli::try_parse_from(["udo", "settings", "set", "uni", "estimate=1h"]).is_ok());
        assert!(Cli::try_parse_from(["udo", "settings", "set"]).is_err());
    }
}
