//! `udo run [NODE] [--with NAME] [--task NODE]`, `udo run --list`: open a
//! node with its run config (`open_with`, or `--with`). udo never starts a
//! timer here: tracking is the script's (`udo track`).

use std::{fmt, path::Path};

use serde::Serialize;

use crate::{
    Res,
    cli::{
        report::Report,
        resolve::{path_text, resolve},
    },
    core::Core,
    model::{NodePath, node::Node, settings::RunName, tree::Tree},
    run::{Event, Library, RunContext, exit_code, launch},
};

#[derive(clap::Args)]
pub struct RunArgs {
    /// Default: the node of the current folder
    pub node: Option<String>,
    /// The run config to use instead of the node's open_with
    #[arg(long, value_name = "NAME")]
    pub with: Option<RunName>,
    /// Opening a container: the task below it the time goes to (one open
    /// task: taken without asking)
    #[arg(long, value_name = "NODE")]
    pub task: Option<String>,
    /// List the run configs (and the one NODE opens with)
    #[arg(long, conflicts_with_all = ["with", "task"])]
    pub list: bool,
}

/// What `udo run` did: which script ran on which node, and its exit code.
/// Quiet as text: the script's own output is what counts.
#[derive(Serialize)]
pub struct Opened {
    pub node: String,
    pub script: String,
    pub code: i32,
}

/// `udo run --list`: the scripts in the library, sorted.
#[derive(Serialize)]
pub struct Listed {
    pub dir: String,
    pub names: Vec<String>,
    /// What NODE opens with (`open_with`), if it names a script.
    pub default: Option<String>,
}

/// Open `args.node` with its run config and wait for the script.
pub async fn run(core: &Core, cwd: &Path, args: &RunArgs) -> Res<Opened> {
    let tree = core.tree();
    let path = resolve(tree, args.node.as_deref(), cwd)?;
    let node = tree.get(&path).ok_or("no such node")?;
    let name = match &args.with {
        Some(name) => name.clone(),
        None => open_with(tree, &path)
            .ok_or_else(|| format!("no run config for {} (set open_with)", node.name()))?,
    };
    let library = Library::load(&tree.run_dir())?;
    let script = library.find(&name)?;
    let task = task_for(tree, &path, args.task.as_deref(), cwd)?;
    let ctx = RunContext::new(tree, Event::Open, &path, Some(&task)).ok_or("no such node")?;

    let status = launch(script, &ctx).await?;
    Ok(Opened {
        node: path_text(tree, &path),
        script: name.to_string(),
        code: exit_code(status),
    })
}

/// The run configs in `run_dir`; with NODE (or inside a udo folder) also
/// what it opens with. A NODE that names nothing is an error; outside udo
/// folders without one, just the list.
pub fn list(core: &Core, cwd: &Path, args: &RunArgs) -> Res<Listed> {
    let tree = core.tree();
    let path = match &args.node {
        Some(node) => Some(resolve(tree, Some(node), cwd)?),
        None => resolve(tree, None, cwd).ok(),
    };
    let library = Library::load(&tree.run_dir())?;
    Ok(Listed {
        dir: library.dir().display().to_string(),
        names: library.names().map(ToString::to_string).collect(),
        default: path
            .and_then(|p| open_with(tree, &p))
            .map(|n| n.to_string()),
    })
}

/// The script `open_with` names for the node at `path`; unset or `none`:
/// None.
fn open_with(tree: &Tree, path: &[usize]) -> Option<RunName> {
    let setting = tree.setting(path, |s| s.open_with.clone())?.value;
    setting.script().cloned()
}

/// The task the time goes to: a task is its own; a container needs
/// `--task` (a task below it), unless exactly one open task is below it.
/// The CLI asks no questions: several open tasks are an error naming them.
fn task_for(tree: &Tree, path: &[usize], task: Option<&str>, cwd: &Path) -> Res<NodePath> {
    let node = tree.get(path).ok_or("no such node")?;
    if node.as_task().is_some() {
        if task.is_some() {
            return Err(format!("{} is a task: --task is for containers", node.name()).into());
        }
        return Ok(path.to_vec());
    }
    if let Some(task) = task {
        let found = resolve(tree, Some(task), cwd)?;
        let below = found.starts_with(path) && tree.get(&found).and_then(Node::as_task).is_some();
        if !below {
            let (task, container) = (path_text(tree, &found), path_text(tree, path));
            return Err(format!("{task} is not a task in {container}").into());
        }
        return Ok(found);
    }
    let open = open_tasks(tree, path);
    match open.as_slice() {
        [one] => Ok(one.clone()),
        [] => Err(format!("no open task in {}", node.name()).into()),
        many => {
            let names: Vec<String> = many
                .iter()
                .filter_map(|p| tree.get(p))
                .map(|n| format!("{:?}", n.name()))
                .collect();
            Err(format!("pick a task: --task {}", names.join(" | ")).into())
        }
    }
}

/// Tasks at any depth below the container at `path` that are not done.
fn open_tasks(tree: &Tree, path: &[usize]) -> Vec<NodePath> {
    tree.rows()
        .into_iter()
        .filter(|r| r.path.starts_with(path) && r.path.len() > path.len())
        .filter(|r| r.node.as_task().is_some_and(|t| t.done_at.is_none()))
        .map(|r| r.path)
        .collect()
}

impl fmt::Display for Opened {
    fn fmt(&self, _: &mut fmt::Formatter<'_>) -> fmt::Result {
        Ok(()) // quiet: the script's own output is what counts
    }
}

impl fmt::Display for Listed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.names.is_empty() {
            return write!(f, "no run configs in {} yet", self.dir);
        }
        write!(f, "run configs in {}", self.dir)?;
        for name in &self.names {
            match &self.default {
                Some(default) if default == name => write!(f, "\n  {name}  (default here)")?,
                _ => write!(f, "\n  {name}")?,
            }
        }
        Ok(())
    }
}

impl Report for Opened {}
impl Report for Listed {}

#[cfg(test)]
mod tests {
    use std::{fs, os::unix::fs::PermissionsExt, path::PathBuf};

    use super::*;
    use crate::{
        model::settings::ContainerSettings,
        test_util::{at, core}, // core: disk_tree, root (tmp): [a, ws (tmp/ws): [b]]
    };

    /// An executable script `name` in `<root>/run` (the default `run_dir`).
    fn script(root: &Path, name: &str, body: &str) -> PathBuf {
        let dir = root.join("run");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    async fn set_open_with(core: &mut Core, path: &[usize], name: &str) {
        let settings = ContainerSettings {
            open_with: Some(name.parse().unwrap()),
            ..Default::default()
        };
        let root = path.is_empty().then(Default::default);
        core.set_settings(path, settings, root).await.unwrap();
    }

    fn open(node: &str) -> RunArgs {
        RunArgs {
            node: Some(node.into()),
            with: None,
            task: None,
            list: false,
        }
    }

    /// A script that writes `$UDO_NODE_NAME $UDO_TASK_NAME` into `out`.
    fn recorder(root: &Path, name: &str) -> PathBuf {
        let out = root.join(format!("{name}.out"));
        let body = format!("echo \"$UDO_NODE_NAME $UDO_TASK_NAME\" > '{}'", out.display());
        script(root, name, &body);
        out
    }

    #[tokio::test]
    async fn a_task_opens_with_its_inherited_open_with() {
        let (tmp, mut core) = core().await;
        let out = recorder(tmp.path(), "editor");
        set_open_with(&mut core, &[], "editor").await;

        let opened = run(&core, tmp.path(), &open("b")).await.unwrap();

        assert_eq!(
            (opened.node.as_str(), opened.script.as_str(), opened.code),
            ("ws/b", "editor", 0)
        );
        assert_eq!(fs::read_to_string(out).unwrap(), "b b\n");
        assert_eq!(opened.to_string(), "");
    }

    #[tokio::test]
    async fn with_overrides_open_with() {
        let (tmp, mut core) = core().await;
        recorder(tmp.path(), "editor");
        let other = recorder(tmp.path(), "other");
        set_open_with(&mut core, &[], "editor").await;
        let mut args = open("a");
        args.with = Some("other".parse().unwrap());

        run(&core, tmp.path(), &args).await.unwrap();

        assert_eq!(fs::read_to_string(other).unwrap(), "a a\n");
    }

    #[tokio::test]
    async fn the_scripts_exit_code_comes_back() {
        let (tmp, core) = core().await;
        script(tmp.path(), "fails", "exit 4");
        let mut args = open("a");
        args.with = Some("fails".parse().unwrap());

        assert_eq!(run(&core, tmp.path(), &args).await.unwrap().code, 4);
    }

    #[tokio::test]
    async fn unset_or_none_says_what_to_set() {
        let (tmp, mut core) = core().await;

        let err = run(&core, tmp.path(), &open("b")).await.err().unwrap();
        assert_eq!(err.to_string(), "no run config for b (set open_with)");

        set_open_with(&mut core, &[], "editor").await;
        set_open_with(&mut core, &[1], "none").await; // ws switches it off
        let err = run(&core, tmp.path(), &open("b")).await.err().unwrap();
        assert_eq!(err.to_string(), "no run config for b (set open_with)");
    }

    #[tokio::test]
    async fn an_unknown_script_lists_the_ones_there_are() {
        let (tmp, mut core) = core().await;
        script(tmp.path(), "editor", "");
        set_open_with(&mut core, &[], "edtior").await;

        let err = run(&core, tmp.path(), &open("b")).await.err().unwrap();

        assert!(err.to_string().ends_with("(have: editor)"), "{err}");
    }

    /// `ws` has one open task (`b`): taken without `--task`.
    #[tokio::test]
    async fn a_container_with_one_open_task_takes_it() {
        let (tmp, mut core) = core().await;
        let out = recorder(tmp.path(), "editor");
        set_open_with(&mut core, &[], "editor").await;

        run(&core, tmp.path(), &open("ws")).await.unwrap();

        assert_eq!(fs::read_to_string(out).unwrap(), "ws b\n");
    }

    /// The root has two open tasks (`a`, `b`): `--task` picks one, without
    /// it the error names them.
    #[tokio::test]
    async fn a_container_with_several_open_tasks_needs_task() {
        let (tmp, mut core) = core().await;
        let out = recorder(tmp.path(), "editor");
        set_open_with(&mut core, &[], "editor").await;

        let err = run(&core, tmp.path(), &open("/")).await.err().unwrap();
        assert_eq!(err.to_string(), "pick a task: --task \"a\" | \"b\"");

        let mut args = open("/");
        args.task = Some("b".into());
        run(&core, tmp.path(), &args).await.unwrap();
        assert_eq!(fs::read_to_string(out).unwrap(), "root b\n");
    }

    #[tokio::test]
    async fn task_must_be_a_task_below_the_container() {
        let (tmp, mut core) = core().await;
        recorder(tmp.path(), "editor");
        set_open_with(&mut core, &[], "editor").await;
        let mut args = open("ws");

        args.task = Some("a".into()); // in the root, not in ws
        let err = run(&core, tmp.path(), &args).await.err().unwrap();
        assert_eq!(err.to_string(), "a is not a task in ws");

        let mut on_task = open("b");
        on_task.task = Some("b".into());
        let err = run(&core, tmp.path(), &on_task).await.err().unwrap();
        assert!(err.to_string().contains("--task is for containers"), "{err}");
    }

    #[tokio::test]
    async fn a_container_without_open_tasks_says_so() {
        let (tmp, mut core) = core().await;
        recorder(tmp.path(), "editor");
        set_open_with(&mut core, &[], "editor").await;
        core.set_done(&[1, 0], true, at(13, 0)).await.unwrap();

        let err = run(&core, tmp.path(), &open("ws")).await.err().unwrap();

        assert_eq!(err.to_string(), "no open task in ws");
    }

    #[tokio::test]
    async fn list_names_the_scripts_and_the_default_here() {
        let (tmp, mut core) = core().await;
        script(tmp.path(), "idea", "");
        script(tmp.path(), "nvim-tmux", "");
        set_open_with(&mut core, &[1], "nvim-tmux").await;
        let mut args = open("b");
        args.list = true;

        let listed = list(&core, tmp.path(), &args).unwrap();

        let dir = tmp.path().join("run");
        assert_eq!(
            listed.to_string(),
            format!("run configs in {}\n  idea\n  nvim-tmux  (default here)", dir.display())
        );
    }

    #[tokio::test]
    async fn list_without_scripts_says_where_they_go() {
        let (tmp, core) = core().await;
        let args = RunArgs {
            list: true,
            ..open("/")
        };

        let listed = list(&core, tmp.path(), &args).unwrap();

        let dir = tmp.path().join("run");
        assert_eq!(listed.to_string(), format!("no run configs in {} yet", dir.display()));
    }
}
