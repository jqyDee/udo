//! `udo session rm ID [--yes]`: remove a session. Soft: hidden from every
//! list, the data stays in `udo.db`.

use super::{id::find, row::SessionDone};
use crate::{Res, core::Core, model::time::Time};

#[derive(clap::Args)]
pub struct RmArgs {
    /// The session: any unique end of its ID
    pub id: String,
    /// Do not ask
    #[arg(long, short)]
    pub yes: bool,
}

/// Remove a session. `confirm` gets the question (the CLI:
/// `confirm::ask_on_terminal`).
pub async fn run(
    core: &Core,
    now: Time,
    args: &RmArgs,
    confirm: &mut dyn FnMut(&str) -> Res<bool>,
) -> Res<SessionDone> {
    let session = find(core, &args.id).await?;
    let done = SessionDone::of("removed", std::slice::from_ref(&session), core, now);
    let question = format!("remove session {}?", done.sessions[0].line());
    if !args.yes && !confirm(&question)? {
        return Err("nothing removed".into());
    }
    core.delete_session(session.id).await?;
    Ok(done)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        cli::commands::session::testing::{nine_to_twelve, now},
        test_util::core,
    };

    #[tokio::test]
    async fn rm_asks_then_removes() {
        let (_tmp, core) = core().await;
        let id = nine_to_twelve(&core).await;
        let mut asked = vec![];
        let mut yes = |q: &str| -> Res<bool> {
            asked.push(q.to_string());
            Ok(true)
        };

        let args = RmArgs {
            id: id.clone(),
            yes: false,
        };
        let done = run(&core, now(), &args, &mut yes).await.unwrap();

        assert_eq!(asked.len(), 1);
        assert!(asked[0].contains(&id), "{}", asked[0]);
        assert!(done.to_string().starts_with("removed:\n"));
        assert!(find(&core, &id).await.is_err()); // gone from every list
    }

    #[tokio::test]
    async fn rm_answered_no_keeps_the_session() {
        let (_tmp, core) = core().await;
        let id = nine_to_twelve(&core).await;
        let mut no = |_: &str| -> Res<bool> { Ok(false) };

        let args = RmArgs {
            id: id.clone(),
            yes: false,
        };
        let Err(err) = run(&core, now(), &args, &mut no).await else {
            panic!("removed without a yes");
        };

        assert_eq!(err.to_string(), "nothing removed");
        assert!(find(&core, &id).await.is_ok());
    }

    #[tokio::test]
    async fn rm_yes_does_not_ask() {
        let (_tmp, core) = core().await;
        let id = nine_to_twelve(&core).await;
        let mut never = |_: &str| -> Res<bool> { panic!("asked despite --yes") };

        let args = RmArgs { id, yes: true };
        assert!(run(&core, now(), &args, &mut never).await.is_ok());
    }
}
