//! `o` / `O`: open the node at the cursor with a run config. `App` only
//! builds the `RunRequest`; the loop runs it (`Flow::Run`).
//!
//! ```text
//! o  on a task       -> Flow::Run
//! o  on a container  -> no open task: Flow::Run without one | else: task picker
//! O                  -> script picker -> as o, with the picked script
//! ```

use std::process::ExitStatus;

use crate::{
    model::{NodePath, node::Node, settings::RunName, tree::Tree},
    run::{Event, Library, RunError, exit_code},
    tui::{
        app::{App, Flow, Mode, RunRequest},
        pick::{PickAction, PickItem, PickValue, Picker},
    },
};

impl App<'_> {
    /// `o`: the node at the cursor with its `open_with`. Every problem is a
    /// toast, and the TUI stays (no flicker for a typo in a setting).
    pub(super) fn open(&mut self) -> Flow {
        let path = self.tree_state.cursor.clone();
        let tree = self.core.tree();
        match tree.open_with(&path) {
            Some(name) => self.open_as(path, name),
            None => {
                let msg = format!(
                    "no run config for {} (set open_with)",
                    node_name(tree, &path)
                );
                self.error(msg);
                Flow::Continue
            }
        }
    }

    /// `O`: the script picker over the library (`open_with` preselected and
    /// marked), then as `o` (`picked` -> `open_as`).
    pub(super) fn pick_script(&mut self) -> Flow {
        let path = self.tree_state.cursor.clone();
        match script_picker(self.core.tree(), path) {
            Ok(picker) => self.mode = Mode::Pick(Box::new(picker)),
            Err(msg) => self.error(msg),
        }
        Flow::Continue
    }

    /// `name` opens the node at `path`. A task: run, its time on itself. A
    /// container never gets a task on its own: without open tasks it runs
    /// without one, else the task picker (no task preselected).
    pub(super) fn open_as(&mut self, path: NodePath, name: RunName) -> Flow {
        let tree = self.core.tree();
        let Some(node) = tree.get(&path) else {
            self.error("no such node");
            return Flow::Continue;
        };
        if node.as_task().is_some() {
            return self.run_on(&path, Some(&path), name);
        }
        let tasks = tree.open_tasks(&path);
        if tasks.is_empty() {
            return self.run_on(&path, None, name);
        }
        self.mode = Mode::Pick(Box::new(task_picker(tree, path, &tasks, name)));
        Flow::Continue
    }

    /// `Flow::Run` for `name` on the node at `node`, its time on the task at
    /// `task` (None: no task, `UDO_TASK_*` unset); a problem (script
    /// missing, not executable, ...) is a toast.
    pub(super) fn run_on(&mut self, node: &[usize], task: Option<&[usize]>, name: RunName) -> Flow {
        match RunRequest::new(self.core.tree(), Event::Open, node, task, name) {
            Ok(request) => Flow::Run(Box::new(request)),
            Err(e) => {
                self.error(e.to_string());
                Flow::Continue
            }
        }
    }

    /// After a create form: the new node's `on_create` as `Flow::Run`.
    /// A problem (script gone, ...) is a toast; the node stays.
    pub(super) fn run_on_create(&mut self, path: &[usize]) -> Flow {
        match RunRequest::on_create(self.core.tree(), path) {
            Ok(Some(request)) => Flow::Run(Box::new(request)),
            Ok(None) => Flow::Continue, // switched off meanwhile
            Err(e) => {
                let name = self
                    .core
                    .tree()
                    .get(path)
                    .map_or("", Node::name)
                    .to_string();
                self.error(format!("{e} ({name} was added)"));
                Flow::Continue
            }
        }
    }

    /// The loop ran `name`'s script: say how it went, then read everything
    /// again (the script may have started a timer through `udo track`).
    pub async fn after_run(&mut self, name: &RunName, result: Result<ExitStatus, RunError>) {
        match result {
            Ok(status) if status.success() => {}
            Ok(status) => self.error(format!("{name} exited with {}", exit_code(status))),
            Err(e) => self.error(e.to_string()),
        }
        self.reload().await;
    }
}

/// `O`'s picker: the library's names, sorted, `open_with` preselected and
/// marked `(default)` (without one: the first). Err: the toast (library
/// unreadable or empty).
fn script_picker(tree: &Tree, path: NodePath) -> Result<Picker, String> {
    let node = tree.get(&path).ok_or("no such node")?;
    let run_dir = tree.run_dir();
    let library = Library::load(&run_dir).map_err(|e| e.to_string())?;
    let default = tree.open_with(&path);
    let items: Vec<PickItem> = library
        .names()
        .map(|name| PickItem {
            label: name.to_string(),
            note: (Some(name) == default.as_ref()).then_some("(default)"),
            value: PickValue::Script(name.clone()),
        })
        .collect();
    if items.is_empty() {
        return Err(format!("no run configs in {}", run_dir.display()));
    }
    let cursor = items.iter().position(|i| i.note.is_some()).unwrap_or(0);
    Ok(Picker {
        title: format!("open {} with", node.name()),
        items,
        cursor,
        action: PickAction::Script { path },
    })
}

/// The task picker: first the container itself (`ws (no task)`,
/// preselected: opening a container never picks a task on its own), then
/// `tasks`, the open tasks below it (at least one), in tree order. Labels
/// are paths below the container (`week 2 / lab 3`), so tasks with the
/// same name stay apart.
fn task_picker(tree: &Tree, container: NodePath, tasks: &[NodePath], script: RunName) -> Picker {
    let itself = PickItem {
        label: node_name(tree, &container).into(),
        note: Some("(no task)"),
        value: PickValue::Task(None),
    };
    let tasks = tasks.iter().map(|task| PickItem {
        label: (container.len() + 1..=task.len())
            .filter_map(|end| tree.get(&task[..end]))
            .map(Node::name)
            .collect::<Vec<_>>()
            .join(" / "),
        note: None,
        value: PickValue::Task(Some(task.clone())),
    });
    Picker {
        title: format!("open {} for", node_name(tree, &container)),
        items: std::iter::once(itself).chain(tasks).collect(),
        cursor: 0,
        action: PickAction::Task { container, script },
    }
}

/// The name of the node at `path`, "" if there is none (for messages).
fn node_name<'t>(tree: &'t Tree, path: &[usize]) -> &'t str {
    tree.get(path).map_or("", Node::name)
}
