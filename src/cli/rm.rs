//! `udo rm [NODE] [--with-folder] [--yes]`: remove a node. Without
//! `--with-folder` only udo forgets it (its files stay); with it, its
//! folders go to the Trash. Asks first unless `--yes`; a timer on the node
//! (or below it) is stopped (a `Core` rule).

use std::{
    fmt,
    io::{BufRead, IsTerminal, Write},
    path::{Path, PathBuf},
};

use serde::Serialize;

use super::{
    report::Report,
    resolve::{path_text, resolve},
    timer::SessionLine,
};
use crate::{
    Res,
    core::Core,
    model::{time::Time, tree::TrashFn},
};

#[derive(clap::Args)]
pub struct RmArgs {
    /// Default: the node of the current folder
    pub node: Option<String>,
    /// Also move the node's folders to the Trash
    #[arg(long)]
    pub with_folder: bool,
    /// Do not ask
    #[arg(long, short)]
    pub yes: bool,
}

/// What `rm` removed.
#[derive(Serialize)]
pub struct Removed {
    pub path: String,
    /// Only unregistered: the folders are still on disk.
    pub files_kept: bool,
    pub trashed: Vec<PathBuf>,
    pub failed: Vec<Failed>,
    pub stopped: Option<SessionLine>,
}

/// A folder that could not be moved to the Trash.
#[derive(Serialize)]
pub struct Failed {
    pub dir: PathBuf,
    pub reason: String,
}

/// Remove `args.node` at `now`. `confirm` gets the question and says yes
/// or no (the CLI: `ask_on_terminal`); `trash` moves a folder away.
pub async fn run(
    core: &mut Core,
    cwd: &Path,
    now: Time,
    args: &RmArgs,
    trash: TrashFn,
    confirm: &mut dyn FnMut(&str) -> Res<bool>,
) -> Res<Removed> {
    let path = resolve(core.tree(), args.node.as_deref(), cwd)?;
    if path.is_empty() {
        return Err("the root cannot be removed".into());
    }
    let shown = path_text(core.tree(), &path);
    let mut ask = |question: String| -> Res<()> {
        if args.yes || confirm(&question)? {
            Ok(())
        } else {
            Err("nothing removed".into())
        }
    };

    if !args.with_folder {
        ask(format!("remove {shown}? (its files stay)"))?;
        let stopped = core.delete(&path, now).await?;
        return Ok(Removed {
            path: shown,
            files_kept: true,
            trashed: vec![],
            failed: vec![],
            stopped: stopped.as_ref().map(|s| SessionLine::of(s, now)),
        });
    }

    let plan = core
        .purge_plan(&path)?
        .ok_or("nothing to delete on disk: use rm without --with-folder")?;
    let mut question = format!("delete {shown} and move to the Trash:");
    for dir in &plan.folders {
        question.push_str(&format!("\n  {}", dir.display()));
    }
    if plan.containers + plan.tasks > 0 {
        question.push_str(&format!(
            "\n({} containers and {} tasks below it go too)",
            plan.containers, plan.tasks
        ));
    }
    ask(question)?;
    let (report, stopped) = core.purge(&plan, trash, now).await?;
    Ok(Removed {
        path: shown,
        files_kept: false,
        trashed: report.trashed,
        failed: report
            .failed
            .into_iter()
            .map(|(dir, reason)| Failed { dir, reason })
            .collect(),
        stopped: stopped.as_ref().map(|s| SessionLine::of(s, now)),
    })
}

/// Ask `question` on the terminal (`[y/N]`, on stderr so stdout keeps only
/// results). Without a terminal there is nobody to ask: `--yes` is needed.
pub fn ask_on_terminal(question: &str) -> Res<bool> {
    if !std::io::stdin().is_terminal() {
        return Err("not a terminal: add --yes to confirm".into());
    }
    eprint!("{question} [y/N] ");
    std::io::stderr().flush()?;
    let mut answer = String::new();
    std::io::stdin().lock().read_line(&mut answer)?;
    Ok(matches!(answer.trim().to_lowercase().as_str(), "y" | "yes"))
}

impl fmt::Display for Removed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.files_kept {
            write!(f, "removed {} (files kept)", self.path)?;
        } else {
            let n = self.trashed.len();
            let folders = if n == 1 { "folder" } else { "folders" };
            write!(f, "deleted {}, {n} {folders} moved to the Trash", self.path)?;
        }
        for failed in &self.failed {
            write!(f, "\ncould not trash {}: {}", failed.dir.display(), failed.reason)?;
        }
        if let Some(stopped) = &self.stopped {
            write!(f, "\nstopped: {}", stopped.text())?;
        }
        Ok(())
    }
}

impl Report for Removed {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::{at, core}; // core: disk_tree, root: [a, ws (tmp/ws): [b]]

    fn rm(node: &str) -> RmArgs {
        RmArgs {
            node: Some(node.into()),
            with_folder: false,
            yes: false,
        }
    }

    /// A trash that trashes nothing (the folders stay for the test).
    fn fake_trash(_: &Path) -> Result<(), String> {
        Ok(())
    }

    /// Says `answer` and remembers the questions.
    fn answering(answer: bool, asked: &mut Vec<String>) -> impl FnMut(&str) -> Res<bool> + '_ {
        move |question| {
            asked.push(question.into());
            Ok(answer)
        }
    }

    #[tokio::test]
    async fn remove_asks_and_keeps_the_files() {
        let (tmp, mut core) = core().await;
        let mut asked = vec![];

        let removed = run(
            &mut core,
            tmp.path(),
            at(14, 0),
            &rm("ws"),
            fake_trash,
            &mut answering(true, &mut asked),
        )
        .await
        .unwrap();

        assert_eq!(asked, vec!["remove ws? (its files stay)"]);
        assert_eq!(removed.to_string(), "removed ws (files kept)");
        assert!(tmp.path().join("ws").is_dir());
        assert!(core.tree().get(&[1]).is_none());
    }

    #[tokio::test]
    async fn no_removes_nothing() {
        let (tmp, mut core) = core().await;
        let mut asked = vec![];

        let result = run(
            &mut core,
            tmp.path(),
            at(14, 0),
            &rm("a"),
            fake_trash,
            &mut answering(false, &mut asked),
        )
        .await;

        assert_eq!(result.err().unwrap().to_string(), "nothing removed");
        assert_eq!(core.tree().get(&[0]).unwrap().name(), "a");
    }

    #[tokio::test]
    async fn yes_does_not_ask() {
        let (tmp, mut core) = core().await;
        let args = RmArgs {
            yes: true,
            ..rm("a")
        };
        let mut never = |_: &str| -> Res<bool> { panic!("asked despite --yes") };

        run(&mut core, tmp.path(), at(14, 0), &args, fake_trash, &mut never)
            .await
            .unwrap();

        assert_eq!(core.tree().get(&[0]).unwrap().name(), "ws");
    }

    #[tokio::test]
    async fn with_folder_lists_the_folders_and_trashes_them() {
        let (tmp, mut core) = core().await;
        let args = RmArgs {
            with_folder: true,
            ..rm("ws")
        };
        let mut asked = vec![];

        let removed = run(
            &mut core,
            tmp.path(),
            at(14, 0),
            &args,
            fake_trash,
            &mut answering(true, &mut asked),
        )
        .await
        .unwrap();

        let ws = tmp.path().join("ws");
        assert_eq!(
            asked,
            vec![format!(
                "delete ws and move to the Trash:\n  {}\n(0 containers and 1 tasks below it go too)",
                ws.display()
            )]
        );
        assert_eq!(removed.trashed, vec![ws]);
        assert_eq!(removed.to_string(), "deleted ws, 1 folder moved to the Trash");
    }

    #[tokio::test]
    async fn with_folder_on_a_node_without_one_is_an_error() {
        let (tmp, mut core) = core().await;
        let args = RmArgs {
            with_folder: true,
            yes: true,
            ..rm("a")
        };

        let result =
            run(&mut core, tmp.path(), at(14, 0), &args, fake_trash, &mut |_: &str| Ok(true)).await;

        assert!(
            result
                .err()
                .unwrap()
                .to_string()
                .contains("without --with-folder")
        );
    }

    #[tokio::test]
    async fn removing_the_timed_task_stops_the_timer() {
        let (tmp, mut core) = core().await;
        core.start(&[0], at(14, 0)).await.unwrap();
        let args = RmArgs {
            yes: true,
            ..rm("a")
        };

        let removed =
            run(&mut core, tmp.path(), at(14, 45), &args, fake_trash, &mut |_: &str| Ok(true))
                .await
                .unwrap();

        assert_eq!(removed.to_string(), "removed a (files kept)\nstopped: a (45m)");
    }

    #[tokio::test]
    async fn the_root_cannot_be_removed() {
        let (tmp, mut core) = core().await;
        let args = RmArgs {
            yes: true,
            ..rm("/")
        };

        let result =
            run(&mut core, tmp.path(), at(14, 0), &args, fake_trash, &mut |_: &str| Ok(true)).await;

        assert!(result.is_err());
    }
}
