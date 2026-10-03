//! A script found and its context built, ready for `launch`: what `o` /
//! `O` hand to the TUI loop (`Flow::Run`), and what `udo run` / `udo add`
//! run themselves.

use std::path::PathBuf;

use crate::{
    Res,
    model::{node::Node, settings::RunName, tree::Tree},
    run::{Event, Library, RunContext},
};

/// A script to run for a node, and what it learns about it (`UDO_*`).
#[derive(Debug, Clone, PartialEq)]
pub struct RunRequest {
    /// For messages: `nvim-tmux exited with 1`.
    pub name: RunName,
    /// The script `name` was found as (`Library::find`).
    pub script: PathBuf,
    /// The node, the task the time goes to, why it runs.
    pub ctx: RunContext,
}

impl RunRequest {
    /// `name` for `event` on the node at `node`, the time on the task at
    /// `task`. The library is read now, so a script added or removed
    /// meanwhile counts. Err: the library cannot be read, `name` is not
    /// (only) there or cannot run (`RunError`, with its hint), or a path
    /// names nothing.
    pub fn new(
        tree: &Tree,
        event: Event,
        node: &[usize],
        task: Option<&[usize]>,
        name: RunName,
    ) -> Res<Self> {
        let library = Library::load(&tree.run_dir())?;
        let script = library.find(&name)?.to_path_buf();
        let ctx = RunContext::new(tree, event, node, task).ok_or("no such node")?;
        Ok(Self { name, script, ctx })
    }

    /// What runs after creating the node at `path`: its `on_create`, the
    /// time on the node if it is a task (a container: no task, its
    /// `UDO_TASK_*` stay unset). None: `on_create` unset or `none`.
    pub fn on_create(tree: &Tree, path: &[usize]) -> Res<Option<Self>> {
        let Some(name) = tree.on_create(path) else {
            return Ok(None);
        };
        let is_task = tree.get(path).and_then(Node::as_task).is_some();
        let task = is_task.then_some(path);
        Self::new(tree, Event::Create, path, task, name).map(Some)
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, os::unix::fs::PermissionsExt, path::Path};

    use super::*;
    use crate::test_util::{container, task, tree_with};

    /// root: [a, ws: [b]], `run_dir` = `run` with an executable `setup`.
    fn tree(run: &Path) -> Tree {
        let script = run.join("setup");
        fs::write(&script, "#!/bin/sh\n").unwrap();
        fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
        let mut t = tree_with(vec![task("a"), container("ws", vec![task("b")])]);
        let root = t.get_mut(&[]).and_then(Node::as_container_mut).unwrap();
        root.root_settings.run_dir = Some(run.to_path_buf());
        t
    }

    fn set_on_create(t: &mut Tree, text: &str) {
        let root = t.get_mut(&[]).and_then(Node::as_container_mut).unwrap();
        root.settings.on_create = Some(text.parse().unwrap());
    }

    #[test]
    fn new_finds_the_script_and_builds_the_context() {
        let run = tempfile::tempdir().unwrap();
        let t = tree(run.path());
        let b = t.get(&[1, 0]).unwrap().id();

        let r = RunRequest::new(
            &t,
            Event::Open,
            &[1],
            Some(&[1, 0]),
            "setup".parse().unwrap(),
        )
        .unwrap();

        assert_eq!(r.name.as_str(), "setup");
        assert_eq!(r.script, run.path().join("setup"));
        assert_eq!(r.ctx.event, Event::Open);
        assert_eq!(r.ctx.node.name, "ws");
        assert_eq!(r.ctx.task.map(|t| t.id), Some(b));
    }

    /// The `RunError` comes through with its hint (the TUI's toast, the
    /// CLI's error).
    #[test]
    fn new_with_an_unknown_name_lists_the_ones_there_are() {
        let run = tempfile::tempdir().unwrap();
        let t = tree(run.path());

        let err = RunRequest::new(&t, Event::Open, &[0], Some(&[0]), "nope".parse().unwrap())
            .unwrap_err()
            .to_string();

        assert!(err.ends_with("(have: setup)"), "{err}");
    }

    #[test]
    fn new_with_a_missing_node_is_an_error() {
        let run = tempfile::tempdir().unwrap();
        let t = tree(run.path());

        let err = RunRequest::new(&t, Event::Open, &[9], None, "setup".parse().unwrap());

        assert_eq!(err.unwrap_err().to_string(), "no such node");
    }

    #[test]
    fn on_create_of_a_task_times_the_task() {
        let run = tempfile::tempdir().unwrap();
        let mut t = tree(run.path());
        set_on_create(&mut t, "setup");
        let b = t.get(&[1, 0]).unwrap().id();

        let r = RunRequest::on_create(&t, &[1, 0])
            .unwrap()
            .expect("a request");

        assert_eq!(r.name.as_str(), "setup");
        assert_eq!(r.ctx.event, Event::Create);
        assert_eq!(r.ctx.node.id, b);
        assert_eq!(r.ctx.task.map(|t| t.id), Some(b));
    }

    #[test]
    fn on_create_of_a_container_has_no_task() {
        let run = tempfile::tempdir().unwrap();
        let mut t = tree(run.path());
        set_on_create(&mut t, "setup");

        let r = RunRequest::on_create(&t, &[1]).unwrap().expect("a request");

        assert_eq!(r.ctx.event, Event::Create);
        assert_eq!(r.ctx.node.name, "ws");
        assert_eq!(r.ctx.task, None);
    }

    #[test]
    fn on_create_unset_or_none_is_nothing() {
        let run = tempfile::tempdir().unwrap();
        let mut t = tree(run.path());
        assert_eq!(RunRequest::on_create(&t, &[0]).unwrap(), None);

        set_on_create(&mut t, "none");
        assert_eq!(RunRequest::on_create(&t, &[0]).unwrap(), None);
    }

    /// Set, but no such script: an error (the CLI's warning, the TUI's
    /// toast), not a silent nothing.
    #[test]
    fn on_create_with_an_unknown_script_is_an_error() {
        let run = tempfile::tempdir().unwrap();
        let mut t = tree(run.path());
        set_on_create(&mut t, "nope");

        let err = RunRequest::on_create(&t, &[0]).unwrap_err().to_string();

        assert!(err.starts_with("no run config \"nope\""), "{err}");
    }
}
