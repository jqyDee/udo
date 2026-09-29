//! `udo session edit ID [--start T] [--end T]`: move a session's times.

use super::{id::find, row::SessionDone};
use crate::{
    Res,
    cli::parse::{SessionTime, session_time},
    core::Core,
    model::{sessions::SessionPatch, time::Time},
};

#[derive(clap::Args)]
pub struct EditArgs {
    /// The session: any unique end of its ID (lists show the last 6)
    pub id: String,
    /// New start
    #[arg(long, value_parser = session_time, allow_hyphen_values = true)]
    pub start: Option<SessionTime>,
    /// New end
    #[arg(long, value_parser = session_time, allow_hyphen_values = true)]
    pub end: Option<SessionTime>,
}

/// Move a session's start and / or end.
pub async fn run(core: &Core, now: Time, args: &EditArgs) -> Res<SessionDone> {
    if args.start.is_none() && args.end.is_none() {
        return Err("nothing to change: give --start and / or --end".into());
    }
    let session = find(core, &args.id).await?;
    let patch = SessionPatch {
        start: args.start.map(|t| t.resolve(now, None)).transpose()?,
        end: args.end.map(|t| t.resolve(now, None)).transpose()?,
    };
    core.edit_session(session.id, patch, now).await?;
    let edited = find(core, &session.id.to_string()).await?;
    Ok(SessionDone::of("edited", &[edited], core, now))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        cli::commands::session::testing::{nine_to_twelve, now, spans, t},
        model::time::Minutes,
        test_util::{core, local},
    };

    #[tokio::test]
    async fn edit_moves_start_and_end() {
        let (_tmp, core) = core().await;
        let id = nine_to_twelve(&core).await;

        let args = EditArgs {
            id,
            start: Some(t(8, 30)),
            end: Some(SessionTime::Ago(Minutes::new(9 * 60))), // 11:00
        };
        let done = run(&core, now(), &args).await.unwrap();

        assert_eq!(spans(&done), vec![(local(8, 30), Some(local(11, 0)))]);
        assert!(done.to_string().starts_with("edited:\n"));
    }

    #[tokio::test]
    async fn edit_without_a_change_is_refused() {
        let (_tmp, core) = core().await;
        let id = nine_to_twelve(&core).await;

        let args = EditArgs {
            id,
            start: None,
            end: None,
        };
        assert!(run(&core, now(), &args).await.is_err());
    }
}
