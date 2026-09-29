//! `udo session cut ID FROM TO`: remove a part of a session, e.g. a break
//! the timer ran through.

use super::{id::find, row::SessionDone};
use crate::{
    Res,
    cli::parse::{SessionTime, session_time},
    core::Core,
    model::time::Time,
};

#[derive(clap::Args)]
pub struct CutArgs {
    /// The session: any unique end of its ID
    pub id: String,
    /// Start of the part to remove
    #[arg(value_parser = session_time, allow_hyphen_values = true)]
    pub from: SessionTime,
    /// Its end, or +D (after FROM)
    #[arg(value_parser = session_time, allow_hyphen_values = true)]
    pub to: SessionTime,
}

/// Remove `[from, to)` from a session.
pub async fn run(core: &Core, now: Time, args: &CutArgs) -> Res<SessionDone> {
    let session = find(core, &args.id).await?;
    let from = args.from.resolve(now, None)?;
    let to = args.to.resolve(now, Some(from))?;
    let left = core.cut_session(session.id, from, to).await?;
    Ok(SessionDone::of("cut", &left, core, now))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        cli::commands::session::{
            id::short_id,
            testing::{nine_to_twelve, now, spans, t},
        },
        model::{sessions::SessionStore, time::Minutes},
        test_util::{core, local},
    };

    #[tokio::test]
    async fn cut_leaves_the_pieces_around_it() {
        let (_tmp, core) = core().await;
        let id = nine_to_twelve(&core).await;

        let args = CutArgs {
            id,
            from: t(10, 0),
            to: SessionTime::After(Minutes::new(45)),
        };
        let done = run(&core, now(), &args).await.unwrap();

        assert_eq!(
            spans(&done),
            vec![
                (local(9, 0), Some(local(10, 0))),
                (local(10, 45), Some(local(12, 0)))
            ]
        );
    }

    #[tokio::test]
    async fn cut_on_a_running_session_keeps_it_running() {
        let (_tmp, mut core) = core().await;
        let running = core.start(&[0], local(19, 0)).await.unwrap();

        let args = CutArgs {
            id: short_id(running.id),
            from: t(19, 15),
            to: t(19, 30),
        };
        let done = run(&core, now(), &args).await.unwrap();

        assert_eq!(done.sessions.last().unwrap().end, None);
        assert!(core.sessions().running().await.unwrap().is_some());
    }
}
