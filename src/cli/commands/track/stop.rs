//! `udo track stop --owner OWNER`

use std::fmt;

use serde::Serialize;

use crate::{
    Res,
    cli::{commands::timer::SessionLine, report::Report},
    core::Core,
    model::{sessions::Owner, time::Time},
};

#[derive(clap::Args)]
pub struct StopArgs {
    /// The owner given to `track start`; not "manual"
    #[arg(long)]
    pub owner: Owner,
}

/// The session that was stopped; `None`: someone else's session runs, or
/// nothing does (not an error).
#[derive(Serialize)]
pub struct Untracked(pub Option<SessionLine>);

/// Stop the session `args.owner` owns at `now`, if it runs.
pub async fn run(core: &Core, now: Time, args: &StopArgs) -> Res<Untracked> {
    let stopped = core.track_stop(&args.owner, now).await?;
    Ok(Untracked(stopped.as_ref().map(|s| SessionLine::of(s, now))))
}

impl fmt::Display for Untracked {
    fn fmt(&self, _: &mut fmt::Formatter<'_>) -> fmt::Result {
        Ok(()) // quiet: hooks run it on every event
    }
}

impl Report for Untracked {}
