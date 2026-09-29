//! `udo session list / add / edit / split / cut / rm`: correct recorded
//! time. One file per command; sessions are named by the end of their ID
//! (`id.rs`) and shown as `SessionRow`s (`row.rs`).

mod add;
mod cut;
mod edit;
mod id;
mod list;
mod rm;
mod row;
mod split;

use std::path::Path;

use crate::{
    Res,
    cli::{confirm::ask_on_terminal, report::emit},
    core::Core,
    model::time,
};

/// `udo session …`. Times: YYYY-MM-DD HH:MM, now, -D (before now), +D
/// (after the first time of a pair); D like 1h30 or 45m.
#[derive(clap::Subcommand)]
pub enum SessionCommand {
    /// List sessions (default: the last 7 days) and their total
    List(list::ListArgs),
    /// Record a session by hand
    Add(add::AddArgs),
    /// Move a session's start or end
    Edit(edit::EditArgs),
    /// Split a session in two
    Split(split::SplitArgs),
    /// Remove a part of a session (e.g. a break the timer ran through)
    Cut(cut::CutArgs),
    /// Remove a session (asks first unless --yes)
    Rm(rm::RmArgs),
}

/// Run `command` at the current time and print its result.
pub async fn run(command: &SessionCommand, core: &Core, cwd: &Path, json: bool) -> Res<()> {
    let now = time::now();
    match command {
        SessionCommand::List(a) => emit(&list::run(core, cwd, now, a).await?, json),
        SessionCommand::Add(a) => emit(&add::run(core, cwd, now, a).await?, json),
        SessionCommand::Edit(a) => emit(&edit::run(core, now, a).await?, json),
        SessionCommand::Split(a) => emit(&split::run(core, now, a).await?, json),
        SessionCommand::Cut(a) => emit(&cut::run(core, now, a).await?, json),
        SessionCommand::Rm(a) => {
            let mut ask = ask_on_terminal;
            emit(&rm::run(core, now, a, &mut ask).await?, json)
        }
    }
}

/// Helpers for the tests of every session command (core: `disk_tree`,
/// root: [a, ws: [b]]; times on 2026-10-15, local).
#[cfg(test)]
mod testing {
    use super::{id::short_id, row::SessionDone};
    use crate::{
        cli::parse::SessionTime,
        core::Core,
        model::time::Time,
        test_util::{dt, local},
    };

    pub fn now() -> Time {
        local(20, 0)
    }

    /// `SessionTime` for 2026-10-15 `h:m` local.
    pub fn t(h: u32, m: u32) -> SessionTime {
        SessionTime::At(dt(2026, 10, 15, h, m))
    }

    /// (start, end) of each resulting session.
    pub fn spans(done: &SessionDone) -> Vec<(Time, Option<Time>)> {
        done.sessions.iter().map(|r| (r.start, r.end)).collect()
    }

    /// A 09:00-12:00 session on task "a"; its short id.
    pub async fn nine_to_twelve(core: &Core) -> String {
        let session = core
            .add_session(&[0], local(9, 0), local(12, 0))
            .await
            .unwrap();
        short_id(session.id)
    }
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use crate::cli::Cli;

    fn parses(args: &[&str]) -> bool {
        let mut argv = vec!["udo", "session"];
        argv.extend_from_slice(args);
        Cli::try_parse_from(argv).is_ok()
    }

    #[test]
    fn session_commands_parse() {
        assert!(parses(&["list"]));
        assert!(parses(&["list", "uni", "--from", "-2h", "--to", "+1h", "--deleted"]));
        assert!(parses(&["add", "lab 3", "2026-10-15 14:00", "+1h30"]));
        assert!(parses(&["edit", "4f9e2c", "--start", "-45m"]));
        assert!(parses(&["split", "4f9e2c", "2026-10-15 12:00"]));
        assert!(parses(&["cut", "4f9e2c", "2026-10-15 12:00", "+45m"]));
        assert!(parses(&["rm", "4f9e2c", "--yes"]));
        // -D as a value, not a flag
        assert!(parses(&["add", "lab 3", "-1h", "now"]));
        assert!(parses(&["split", "4f9e2c", "-30m"]));
        assert!(parses(&["cut", "4f9e2c", "-1h", "-45m"]));
        assert!(parses(&["edit", "4f9e2c", "--end", "-5m"]));
    }

    #[test]
    fn missing_or_wrong_arguments_are_refused() {
        assert!(!parses(&["add", "lab 3", "2026-10-15 14:00"])); // no END
        assert!(!parses(&["add", "lab 3", "14:00", "15:00"])); // no date
        assert!(!parses(&["list", "--all", "--from", "-1h"]));
        assert!(!parses(&["cut", "4f9e2c", "now"]));
        assert!(!parses(&["rm"]));
    }
}
