//! `udo session add NODE START END`: record a session by hand.

use std::path::Path;

use super::row::SessionDone;
use crate::{
    Res,
    cli::{
        parse::{SessionTime, session_time},
        resolve::resolve,
    },
    core::Core,
    model::time::Time,
};

#[derive(clap::Args)]
pub struct AddArgs {
    /// The task
    pub node: String,
    /// YYYY-MM-DD HH:MM, now or -D (before now)
    #[arg(value_parser = session_time, allow_hyphen_values = true)]
    pub start: SessionTime,
    /// Like START, or +D (after START)
    #[arg(value_parser = session_time, allow_hyphen_values = true)]
    pub end: SessionTime,
}

/// Record a session on `args.node` by hand.
pub async fn run(core: &Core, cwd: &Path, now: Time, args: &AddArgs) -> Res<SessionDone> {
    let path = resolve(core.tree(), Some(&args.node), cwd)?;
    let start = args.start.resolve(now, None)?;
    let end = args.end.resolve(now, Some(start))?;
    let session = core.add_session(&path, start, end, now).await?;
    Ok(SessionDone::of("added", &[session], core, now))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        cli::commands::session::testing::{nine_to_twelve, now, spans, t},
        model::time::Minutes,
        test_util::{core, local},
    };

    fn add_args(node: &str, start: SessionTime, end: SessionTime) -> AddArgs {
        AddArgs {
            node: node.into(),
            start,
            end,
        }
    }

    #[tokio::test]
    async fn add_records_a_session_and_shows_it() {
        let (tmp, core) = core().await;

        let done = run(&core, tmp.path(), now(), &add_args("ws/b", t(14, 0), t(15, 12)))
            .await
            .unwrap();

        assert_eq!(spans(&done), vec![(local(14, 0), Some(local(15, 12)))]);
        let id = &done.sessions[0].id;
        assert_eq!(
            done.to_string(),
            format!("added:\n{id}  thu 2026-10-15  14:00-15:12  1h12  ws/b")
        );
    }

    #[tokio::test]
    async fn add_end_plus_d_counts_from_the_start() {
        let (tmp, core) = core().await;
        let end = SessionTime::After(Minutes::new(90));

        let done = run(&core, tmp.path(), now(), &add_args("a", t(14, 0), end))
            .await
            .unwrap();

        assert_eq!(spans(&done), vec![(local(14, 0), Some(local(15, 30)))]);
    }

    #[tokio::test]
    async fn add_start_plus_d_is_refused() {
        let (tmp, core) = core().await;
        let start = SessionTime::After(Minutes::new(90));

        let result = run(&core, tmp.path(), now(), &add_args("a", start, t(15, 0))).await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn add_passes_the_store_refusal_through() {
        let (tmp, core) = core().await;
        nine_to_twelve(&core).await;

        let Err(err) = run(&core, tmp.path(), now(), &add_args("a", t(11, 0), t(13, 0))).await
        else {
            panic!("an overlapping session was added");
        };

        assert_eq!(err.to_string(), "overlaps another session");
    }
}
