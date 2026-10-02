//! `udo session split ID AT`: one session becomes two.

use super::{id::find, row::SessionDone};
use crate::{
    Res,
    cli::parse::{SessionTime, session_time},
    core::{Core, SPLIT_AT_EDGE},
    model::time::Time,
};

#[derive(clap::Args)]
pub struct SplitArgs {
    /// The session: any unique end of its ID
    pub id: String,
    /// Where to split it
    #[arg(value_parser = session_time, allow_hyphen_values = true)]
    pub at: SessionTime,
}

/// Split a session in two at `args.at`.
pub async fn run(core: &Core, now: Time, args: &SplitArgs) -> Res<SessionDone> {
    let session = find(core, &args.id).await?;
    let at = args.at.resolve(now, None)?;
    let (a, b) = core
        .split_session(session.id, at)
        .await?
        .ok_or(SPLIT_AT_EDGE)?;
    Ok(SessionDone::of("split", &[a, b], core, now))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        cli::{
            commands::session::testing::{nine_to_twelve, now, spans, t},
            report::render,
        },
        test_util::{core, local},
    };

    #[tokio::test]
    async fn split_shows_both_pieces() {
        let (_tmp, core) = core().await;
        let id = nine_to_twelve(&core).await;

        let done = run(&core, now(), &SplitArgs { id, at: t(10, 0) })
            .await
            .unwrap();

        assert_eq!(
            spans(&done),
            vec![
                (local(9, 0), Some(local(10, 0))),
                (local(10, 0), Some(local(12, 0)))
            ]
        );
    }

    #[tokio::test]
    async fn split_outside_the_session_is_refused() {
        let (_tmp, core) = core().await;
        let id = nine_to_twelve(&core).await;

        let Err(err) = run(&core, now(), &SplitArgs { id, at: t(13, 0) }).await else {
            panic!("split outside the session");
        };

        assert_eq!(err.to_string(), "time outside of session time");
    }

    #[tokio::test]
    async fn split_on_the_start_is_refused() {
        let (_tmp, core) = core().await;
        let id = nine_to_twelve(&core).await;

        assert!(
            run(&core, now(), &SplitArgs { id, at: t(9, 0) })
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn done_json_has_the_action_and_full_rows() {
        let (_tmp, core) = core().await;
        let id = nine_to_twelve(&core).await;

        let done = run(&core, now(), &SplitArgs { id, at: t(10, 0) })
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_str(&render(&done, true).unwrap()).unwrap();

        assert_eq!(json["action"], "split");
        assert_eq!(json["sessions"].as_array().unwrap().len(), 2);
        assert_eq!(json["sessions"][0]["full_id"].as_str().unwrap().len(), 36);
    }
}
