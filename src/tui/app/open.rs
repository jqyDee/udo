//! `o` / `O`: open the node at the cursor with a run config. `App` only
//! builds the `RunRequest`; the loop runs it (`Flow::Run`).
//!
//! ```text
//! o  on a task       -> Flow::Run
//! o  on a container  -> no open task: toast | one: Flow::Run | more: task picker
//! O                  -> script picker -> as o, with the picked script
//! ```

use std::process::ExitStatus;

use crate::{
    model::{NodePath, node::Node, settings::RunName, tree::Tree},
    run::{Event, Library, RunContext, RunError, exit_code},
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
                let msg = format!("no run config for {} (set open_with)", node_name(tree, &path));
                self.error(msg);
                Flow::Continue
            }
        }
    }

    /// `O`: the script picker over the library (`open_with` preselected and
    /// marked), then as `o` (`picked` -> `open_as`). A container without an
    /// open task is a toast before picking: the pick would lead nowhere.
    pub(super) fn pick_script(&mut self) -> Flow {
        let path = self.tree_state.cursor.clone();
        match script_picker(self.core.tree(), path) {
            Ok(picker) => self.mode = Mode::Pick(Box::new(picker)),
            Err(msg) => self.error(msg),
        }
        Flow::Continue
    }

    /// `name` opens the node at `path`. A task: run, its time on itself. A
    /// container: the time needs a task below it, so none is a toast, one
    /// is taken, more open the task picker.
    pub(super) fn open_as(&mut self, path: NodePath, name: RunName) -> Flow {
        let tree = self.core.tree();
        let Some(node) = tree.get(&path) else {
            self.error("no such node");
            return Flow::Continue;
        };
        if node.as_task().is_some() {
            return self.run_on(&path, &path, name);
        }
        let tasks = tree.open_tasks(&path);
        match tasks.as_slice() {
            [] => {
                let msg = no_open_task(node);
                self.error(msg);
                Flow::Continue
            }
            [one] => {
                let one = one.clone();
                self.run_on(&path, &one, name)
            }
            _ => {
                self.mode = Mode::Pick(Box::new(task_picker(tree, path, &tasks, name)));
                Flow::Continue
            }
        }
    }

    /// `Flow::Run` for `name` on the node at `node`, its time on the task at
    /// `task`; a problem (script missing, not executable, ...) is a toast.
    pub(super) fn run_on(&mut self, node: &[usize], task: &[usize], name: RunName) -> Flow {
        match run_request(self.core.tree(), node, task, name) {
            Ok(request) => Flow::Run(Box::new(request)),
            Err(msg) => {
                self.error(msg);
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

/// What runs when `name` opens the node at `node` with its time on the
/// task at `task` (the node itself, or one picked below a container).
fn run_request(
    tree: &Tree,
    node: &[usize],
    task: &[usize],
    name: RunName,
) -> Result<RunRequest, String> {
    let library = Library::load(&tree.run_dir()).map_err(|e| e.to_string())?;
    let script = library
        .find(&name)
        .map_err(|e| e.to_string())?
        .to_path_buf();
    let ctx = RunContext::new(tree, Event::Open, node, Some(task)).ok_or("no such node")?;
    Ok(RunRequest { name, script, ctx })
}

/// `O`'s picker: the library's names, sorted, `open_with` preselected and
/// marked `(default)` (without one: the first). Err: the toast (library
/// unreadable or empty, a container without an open task).
fn script_picker(tree: &Tree, path: NodePath) -> Result<Picker, String> {
    let node = tree.get(&path).ok_or("no such node")?;
    if node.as_container().is_some() && tree.open_tasks(&path).is_empty() {
        return Err(no_open_task(node));
    }
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

/// The task picker over `tasks`, the open tasks below `container` (at
/// least two), the earliest due preselected (a tie: the first in the
/// tree). Labels are paths below the container (`week 2 / lab 3`), so
/// tasks with the same name stay apart.
fn task_picker(tree: &Tree, container: NodePath, tasks: &[NodePath], script: RunName) -> Picker {
    let items = tasks
        .iter()
        .map(|task| PickItem {
            label: (container.len() + 1..=task.len())
                .filter_map(|end| tree.get(&task[..end]))
                .map(Node::name)
                .collect::<Vec<_>>()
                .join(" / "),
            note: None,
            value: PickValue::Task(task.clone()),
        })
        .collect();
    // `min_by_key` keeps the first of equal keys: tree order breaks ties
    let cursor = tasks
        .iter()
        .enumerate()
        .min_by_key(|(_, t)| tree.get(t).and_then(Node::as_task).map(|t| t.due_date))
        .map_or(0, |(i, _)| i);
    Picker {
        title: format!("open {} for", node_name(tree, &container)),
        items,
        cursor,
        action: PickAction::Task { container, script },
    }
}

/// `no open task in cs101`
fn no_open_task(node: &Node) -> String {
    format!("no open task in {}", node.name())
}

/// The name of the node at `path`, "" if there is none (for messages).
fn node_name<'t>(tree: &'t Tree, path: &[usize]) -> &'t str {
    tree.get(path).map_or("", Node::name)
}
