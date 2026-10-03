//! `udo add task|project|workspace NODE [--no-run]`: the last part of NODE
//! is the new name, the rest names its parent (see `resolve_parent`).
//! Then the new node's `on_create` runs (inherited from the parent),
//! unless `--no-run`. A failing script is a warning: the node is there, so
//! `add` still succeeds (a caller must not retry into "name exists").

mod container;
mod task;

use std::{fmt, path::Path, path::PathBuf};

use chrono::Local;
use serde::Serialize;

use crate::{
    Res,
    cli::report::{Report, emit},
    core::Core,
    model::{container::ContainerKind, tree::Tree},
    run::{RunRequest, Stdout, exit_code, launch},
};

#[derive(clap::Subcommand)]
pub enum AddCommand {
    /// Add a task, e.g. `udo add task "uni/cs/lab 4" --due "fri 22:00"`
    Task(task::AddTaskArgs),
    /// Add a project, e.g. `udo add project uni/cs`
    Project(container::AddContainerArgs),
    /// Add a workspace, e.g. `udo add workspace uni --dir ~/uni`
    Workspace(container::AddContainerArgs),
}

/// `--no-run`, flattened into every `add`'s args.
#[derive(clap::Args)]
pub struct RunFlag {
    /// Don't run the on_create script for the new node
    #[arg(long)]
    pub no_run: bool,
}

impl AddCommand {
    /// `--no-run` was given.
    fn no_run(&self) -> bool {
        match self {
            AddCommand::Task(a) => a.run.no_run,
            AddCommand::Project(a) | AddCommand::Workspace(a) => a.run.no_run,
        }
    }
}

/// What `on_create` did for the new node (`--json`: `ran`).
#[derive(Debug, Serialize, PartialEq)]
pub struct Ran {
    pub script: String,
    /// The script's exit code (a signal: 128 + n); None: it did not start
    /// (unknown name, not executable, ...).
    pub code: Option<i32>,
    /// Why it failed (did not start, or exit != 0); None: it went fine.
    pub error: Option<String>,
}

/// What `add` created.
#[derive(Serialize)]
pub struct Added {
    /// `task`, `project` or `workspace`.
    pub what: String,
    pub path: String,
    pub dir: Option<PathBuf>,
    /// `on_create`, if one is set and `--no-run` was not given.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ran: Option<Ran>,
}

/// Run `command` and print what it added; a failed `on_create` as a
/// warning on stderr (stdout stays the report, `--json` stays JSON).
pub async fn run(command: &AddCommand, core: &mut Core, cwd: &Path, json: bool) -> Res<()> {
    let added = add(command, core, cwd, Stdout::for_json(json)).await?;
    if let Some(warning) = added.warning() {
        eprintln!("warning: {warning}");
    }
    emit(&added, json)
}

/// `run` without printing: create the node, then its `on_create` (its
/// stdout where `stdout` says). Err: only if nothing was created.
async fn add(command: &AddCommand, core: &mut Core, cwd: &Path, stdout: Stdout) -> Res<Added> {
    let (path, mut added) = match command {
        AddCommand::Task(a) => task::run(core, cwd, Local::now().naive_local(), a).await?,
        AddCommand::Project(a) => container::run(core, cwd, a, ContainerKind::Project).await?,
        AddCommand::Workspace(a) => container::run(core, cwd, a, ContainerKind::Workspace).await?,
    };
    if !command.no_run() {
        added.ran = on_create(core.tree(), &path, stdout).await;
    }
    Ok(added)
}

/// Run `on_create` for the new node at `path`, the terminal handed over
/// (its stdout where `stdout` says). None: nothing set (or `none`). Never
/// `Err`: the node is there whatever the script does, so a problem is only
/// reported (`Ran::error`).
async fn on_create(tree: &Tree, path: &[usize], stdout: Stdout) -> Option<Ran> {
    // first: the name is known even if the request fails (unknown script)
    let script = tree.on_create(path)?.to_string();
    let failed = |script, error: String| Ran {
        script,
        code: None,
        error: Some(error),
    };
    let request = match RunRequest::on_create(tree, path) {
        Ok(Some(request)) => request,
        Ok(None) => return None, // not reached: on_create is set
        Err(e) => return Some(failed(script, e.to_string())),
    };
    match launch(&request.script, &request.ctx, stdout).await {
        Ok(status) => {
            let code = exit_code(status);
            let error = (code != 0).then(|| format!("exited with {code}"));
            Some(Ran {
                script,
                code: Some(code),
                error,
            })
        }
        Err(e) => Some(failed(script, e.to_string())),
    }
}

impl Added {
    /// `on_create typst-setup: exited with 1 (ws/lab 4 was added)`; None:
    /// nothing ran, or it went fine.
    fn warning(&self) -> Option<String> {
        let ran = self.ran.as_ref()?;
        let error = ran.error.as_ref()?;
        Some(format!("on_create {}: {error} ({} was added)", ran.script, self.path))
    }
}

/// `added task ws/lab 4`, its folder, and the `on_create` that ran fine
/// (a failed one is the warning instead).
impl fmt::Display for Added {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "added {} {}", self.what, self.path)?;
        if let Some(dir) = &self.dir {
            write!(f, "\nfolder: {}", dir.display())?;
        }
        if let Some(ran) = self.ran.as_ref().filter(|r| r.error.is_none()) {
            write!(f, "\non_create: {}", ran.script)?;
        }
        Ok(())
    }
}

impl Report for Added {}

#[cfg(test)]
mod tests {
    use std::fs;

    use clap::Parser;

    use super::*;
    use crate::{
        cli::{Cli, commands::Command, report::render},
        model::settings::ContainerSettings,
        // core: disk_tree, root (tmp): [a, ws (tmp/ws): [b]]
        test_util::{core, recorder, run_script},
    };

    async fn set_on_create(core: &mut Core, path: &[usize], name: &str) {
        let settings = ContainerSettings {
            on_create: Some(name.parse().unwrap()),
            ..Default::default()
        };
        let root = path.is_empty().then(Default::default);
        core.set_settings(path, settings, root).await.unwrap();
    }

    /// `udo add ARGS…` as parsed from the command line.
    fn command(args: &[&str]) -> AddCommand {
        let cli = Cli::try_parse_from(["udo", "add"].iter().chain(args)).unwrap();
        match cli.command {
            Some(Command::Add(c)) => c,
            _ => panic!("not an add"),
        }
    }

    #[tokio::test]
    async fn add_task_runs_on_create_for_it() {
        let (tmp, mut core) = core().await;
        let out = recorder(tmp.path(), "setup");
        set_on_create(&mut core, &[], "setup").await;

        let added = add(&command(&["task", "ws/lab 4"]), &mut core, tmp.path(), Stdout::Inherit)
            .await
            .unwrap();

        assert_eq!(fs::read_to_string(out).unwrap(), "create lab 4 lab 4\n");
        let ran = Ran {
            script: "setup".into(),
            code: Some(0),
            error: None,
        };
        assert_eq!(added.ran, Some(ran));
        assert_eq!(added.warning(), None);
        assert!(added.to_string().ends_with("\non_create: setup"), "{added}");
    }

    /// A container has no task: `UDO_TASK_NAME` is empty.
    #[tokio::test]
    async fn add_project_runs_on_create_without_a_task() {
        let (tmp, mut core) = core().await;
        let out = recorder(tmp.path(), "setup");
        set_on_create(&mut core, &[], "setup").await;

        add(&command(&["project", "ws/cs"]), &mut core, tmp.path(), Stdout::Inherit)
            .await
            .unwrap();

        assert_eq!(fs::read_to_string(out).unwrap(), "create cs \n");
    }

    #[tokio::test]
    async fn no_run_skips_it() {
        let (tmp, mut core) = core().await;
        let out = recorder(tmp.path(), "setup");
        set_on_create(&mut core, &[], "setup").await;

        let added = add(
            &command(&["task", "ws/lab 4", "--no-run"]),
            &mut core,
            tmp.path(),
            Stdout::Inherit,
        )
        .await
        .unwrap();

        assert!(!out.exists(), "it ran");
        assert_eq!(added.ran, None);
        assert!(core.tree().get(&[1, 1]).is_some(), "the task is there");
    }

    /// `--no-run` exists on the container commands too.
    #[test]
    fn no_run_parses_for_every_add() {
        for kind in ["task", "project", "workspace"] {
            assert!(command(&[kind, "x", "--no-run"]).no_run(), "{kind}");
            assert!(!command(&[kind, "x"]).no_run(), "{kind}");
        }
    }

    /// The root sets it, `ws` switches it off for what is created in it.
    #[tokio::test]
    async fn none_below_switches_it_off() {
        let (tmp, mut core) = core().await;
        let out = recorder(tmp.path(), "setup");
        set_on_create(&mut core, &[], "setup").await;
        set_on_create(&mut core, &[1], "none").await;

        let added = add(&command(&["task", "ws/lab 4"]), &mut core, tmp.path(), Stdout::Inherit)
            .await
            .unwrap();

        assert!(!out.exists());
        assert_eq!(added.ran, None);
    }

    #[tokio::test]
    async fn without_on_create_nothing_runs_and_nothing_is_said() {
        let (tmp, mut core) = core().await;

        let added = add(&command(&["task", "ws/lab 4"]), &mut core, tmp.path(), Stdout::Inherit)
            .await
            .unwrap();

        assert_eq!(added.ran, None);
        assert_eq!(added.to_string(), "added task ws/lab 4");
    }

    #[tokio::test]
    async fn a_failing_script_keeps_the_node_and_warns() {
        let (tmp, mut core) = core().await;
        run_script(tmp.path(), "setup", "exit 3");
        set_on_create(&mut core, &[], "setup").await;

        let added = add(&command(&["task", "ws/lab 4"]), &mut core, tmp.path(), Stdout::Inherit)
            .await
            .unwrap();

        assert!(core.tree().get(&[1, 1]).is_some(), "the task is there");
        assert_eq!(added.ran.as_ref().unwrap().code, Some(3));
        assert_eq!(added.warning().unwrap(), "on_create setup: exited with 3 (ws/lab 4 was added)");
        assert_eq!(added.to_string(), "added task ws/lab 4", "no success line");
    }

    #[tokio::test]
    async fn an_unknown_script_keeps_the_node_and_warns() {
        let (tmp, mut core) = core().await;
        run_script(tmp.path(), "setup", "");
        set_on_create(&mut core, &[], "stup").await;

        let added = add(&command(&["task", "ws/lab 4"]), &mut core, tmp.path(), Stdout::Inherit)
            .await
            .unwrap();

        assert!(core.tree().get(&[1, 1]).is_some(), "the task is there");
        let ran = added.ran.as_ref().unwrap();
        assert_eq!((ran.script.as_str(), ran.code), ("stup", None));
        let warning = added.warning().unwrap();
        assert!(warning.starts_with("on_create stup: no run config"), "{warning}");
        assert!(warning.contains("(have: setup)"), "{warning}");
        assert!(warning.ends_with("(ws/lab 4 was added)"), "{warning}");
    }

    /// The create itself failing is still an error, and nothing runs.
    #[tokio::test]
    async fn a_failed_create_runs_nothing() {
        let (tmp, mut core) = core().await;
        let out = recorder(tmp.path(), "setup");
        set_on_create(&mut core, &[], "setup").await;

        let err = add(&command(&["task", "ws/b"]), &mut core, tmp.path(), Stdout::Inherit).await;

        assert!(err.is_err(), "b exists");
        assert!(!out.exists());
    }

    #[tokio::test]
    async fn json_has_ran_only_when_something_ran() {
        let (tmp, mut core) = core().await;
        recorder(tmp.path(), "setup");

        let quiet = add(&command(&["task", "ws/lab 4"]), &mut core, tmp.path(), Stdout::Inherit)
            .await
            .unwrap();
        assert!(!render(&quiet, true).unwrap().contains("\"ran\""));

        set_on_create(&mut core, &[], "setup").await;
        let ran = add(&command(&["task", "ws/lab 5"]), &mut core, tmp.path(), Stdout::Inherit)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_str(&render(&ran, true).unwrap()).unwrap();
        assert_eq!(json["ran"], serde_json::json!({"script": "setup", "code": 0, "error": null}));
    }
}
