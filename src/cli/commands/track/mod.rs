//! `udo track start / stop / run`: the timer for programs (tmux hooks,
//! editor wrappers, run configs). Quiet: hooks call it on every event, so
//! the text form is empty; `--json` gives the session. Exit 0 also when
//! nothing happens (someone else's session, nothing running); 1 for real
//! errors (unknown or done task, `--owner manual`); 2 for arguments clap
//! refuses (an invalid `--source` / `--owner`). `run` exits with its
//! child's code; `run --detach` with 0 once the session is started.

use std::path::Path;

use crate::{Res, cli::report::emit, core::Core, model::time, run::Stdout};

mod run;
mod start;
mod stop;

/// `udo track …`. Owner: who may stop the session (one per program
/// instance, e.g. tmux:udo-<task id>); source: which kind of program.
#[derive(clap::Subcommand)]
pub enum TrackCommand {
    /// Start timing a task for a program (takes over a running timer)
    Start(start::StartArgs),
    /// Stop the program's own session (anyone else's: nothing happens)
    Stop(stop::StopArgs),
    /// Time a task while a program runs (… -- idea --wait DIR); exits with its code
    Run(run::RunArgs),
}

/// Run `command` at the current time and print its result.
pub async fn run(command: &TrackCommand, core: &mut Core, cwd: &Path, json: bool) -> Res<()> {
    match command {
        TrackCommand::Start(a) => emit(&start::run(core, cwd, time::now(), a).await?, json),
        TrackCommand::Stop(a) => emit(&stop::run(core, time::now(), a).await?, json),
        TrackCommand::Run(a) if a.detach => {
            emit(&run::detach(core, cwd, time::now(), a).await?, json)
        }
        TrackCommand::Run(a) => {
            let ran = run::run(core, cwd, time::now, a, Stdout::for_json(json)).await?;
            emit(&ran, json)?;
            if ran.code != 0 {
                // a wrapper passes its child's code on; nothing left to
                // flush (the stop is committed)
                std::process::exit(ran.code);
            }
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::{
        start::{self, StartArgs},
        stop::{self, StopArgs},
    };
    use crate::{
        cli::{Cli, report::render},
        core::Core,
        model::sessions::{Owner, SessionStore},
        test_util::{at, core}, // core: disk_tree, root: [a, ws: [b]]
    };

    /// `--task id:<b> --source tmux --owner <owner>`
    fn start_b(core: &Core, owner: &str) -> StartArgs {
        StartArgs {
            task: format!("id:{}", core.tree().get(&[1, 0]).unwrap().id()),
            source: "tmux".parse().unwrap(),
            owner: owner.parse().unwrap(),
        }
    }

    fn stop_by(owner: &str) -> StopArgs {
        StopArgs {
            owner: owner.parse().unwrap(),
        }
    }

    fn json(report: &impl crate::cli::report::Report) -> serde_json::Value {
        serde_json::from_str(&render(report, true).unwrap()).unwrap()
    }

    #[tokio::test]
    async fn start_by_id_is_quiet_and_json_has_the_session() {
        let (tmp, mut core) = core().await;
        let args = start_b(&core, "tmux:a");

        let tracked = start::run(&mut core, tmp.path(), at(14, 0), &args)
            .await
            .unwrap();

        assert_eq!(tracked.to_string(), ""); // `emit` prints nothing
        let json = json(&tracked);
        assert_eq!(json["task"], "b");
        assert_eq!(json["source"], "tmux");
        assert_eq!(json["owner"], "tmux:a");
        assert!(core.sessions().running().await.unwrap().is_some());
    }

    #[tokio::test]
    async fn stop_by_its_owner_returns_the_session() {
        let (tmp, mut core) = core().await;
        let args = start_b(&core, "tmux:a");
        start::run(&mut core, tmp.path(), at(14, 0), &args)
            .await
            .unwrap();

        let stopped = stop::run(&core, at(14, 45), &stop_by("tmux:a"))
            .await
            .unwrap();

        assert_eq!(stopped.to_string(), "");
        assert_eq!(json(&stopped)["minutes"], 45);
        assert_eq!(core.sessions().running().await.unwrap(), None);
    }

    /// Someone else's session: `Ok` (exit 0), JSON `null`, it keeps running.
    #[tokio::test]
    async fn stop_by_another_owner_is_ok_and_null() {
        let (tmp, mut core) = core().await;
        let args = start_b(&core, "tmux:a");
        start::run(&mut core, tmp.path(), at(14, 0), &args)
            .await
            .unwrap();

        let stopped = stop::run(&core, at(15, 0), &stop_by("tmux:b"))
            .await
            .unwrap();

        assert!(json(&stopped).is_null());
        assert!(core.sessions().running().await.unwrap().is_some());
    }

    /// `Err` -> exit 1 (`main`).
    #[tokio::test]
    async fn manual_owner_is_an_error() {
        let (tmp, mut core) = core().await;
        let mut args = start_b(&core, "tmux:a");
        args.owner = Owner::manual();

        assert!(
            start::run(&mut core, tmp.path(), at(14, 0), &args)
                .await
                .is_err()
        );
        assert!(
            stop::run(&core, at(14, 0), &stop_by("manual"))
                .await
                .is_err()
        );
        assert_eq!(core.sessions().running().await.unwrap(), None);
    }

    #[tokio::test]
    async fn an_unknown_task_is_an_error() {
        let (tmp, mut core) = core().await;
        let mut args = start_b(&core, "tmux:a");
        args.task = "lab 9".into();

        let err = start::run(&mut core, tmp.path(), at(14, 0), &args)
            .await
            .err()
            .unwrap();

        assert_eq!(err.to_string(), "not found: \"lab 9\"");
    }

    /// Invalid values never reach `run`: clap refuses them (exit 2).
    #[test]
    fn clap_refuses_invalid_arguments() {
        let parse = |args: &[&str]| {
            let full = ["udo", "track"].iter().chain(args);
            Cli::try_parse_from(full).is_ok()
        };

        assert!(parse(&[
            "start", "--task", "a", "--source", "tmux", "--owner", "tmux:a"
        ]));
        assert!(parse(&["stop", "--owner", "tmux:a"]));
        assert!(!parse(&[
            "start", "--task", "a", "--source", "Tmux", "--owner", "tmux:a"
        ]));
        assert!(!parse(&[
            "start", "--task", "a", "--source", "tmux", "--owner", "a b"
        ]));
        assert!(!parse(&["start", "--source", "tmux", "--owner", "tmux:a"])); // no --task
        assert!(!parse(&["stop"])); // no --owner
    }

    /// Everything after `--` is the command, its own flags included.
    #[test]
    fn run_takes_the_command_after_the_dashes() {
        let base = [
            "udo", "track", "run", "--task", "a", "--source", "idea", "--owner", "idea:1",
        ];
        let parse = |rest: &[&str]| Cli::try_parse_from(base.iter().chain(rest));

        assert!(parse(&["--", "idea", "--wait", "/tmp"]).is_ok());
        assert!(parse(&["--"]).is_err()); // no command
        assert!(parse(&[]).is_err());
        assert!(parse(&["idea"]).is_err()); // not after `--`
        assert!(parse(&["--detach", "--", "idea"]).is_ok());
        assert!(parse(&["--started", "--", "idea"]).is_ok()); // the helper
        assert!(parse(&["--detach", "--started", "--", "idea"]).is_err());
    }
}
