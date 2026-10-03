//! `udo track start --task NODE --source NAME --owner OWNER`

use std::{fmt, path::Path};

use serde::Serialize;

use crate::{
    Res,
    cli::{commands::timer::SessionLine, report::Report, resolve::resolve},
    core::Core,
    model::{
        sessions::{Owner, SessionSource},
        time::Time,
    },
};

#[derive(clap::Args)]
pub struct StartArgs {
    /// The task, as NODE everywhere (scripts: id:$UDO_TASK_ID)
    #[arg(long)]
    pub task: String,
    /// What kind of program: a-z, 0-9, - (tmux, idea, …)
    #[arg(long)]
    pub source: SessionSource,
    /// Which instance may stop it, e.g. tmux:udo-<task id>; not "manual"
    #[arg(long)]
    pub owner: Owner,
}

/// The session now running (yours, or the one already on that task).
#[derive(Serialize)]
pub struct Tracked(pub SessionLine);

/// Start timing `args.task` for a program at `now`.
pub async fn run(core: &mut Core, cwd: &Path, now: Time, args: &StartArgs) -> Res<Tracked> {
    let path = resolve(core.tree(), Some(&args.task), cwd)?;
    let session = core
        .track_start(&path, args.source.clone(), args.owner.clone(), now)
        .await?;
    Ok(Tracked(SessionLine::of(&session, now)))
}

impl fmt::Display for Tracked {
    fn fmt(&self, _: &mut fmt::Formatter<'_>) -> fmt::Result {
        Ok(()) // quiet: hooks run it on every event
    }
}

impl Report for Tracked {}
