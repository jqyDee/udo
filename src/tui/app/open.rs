//! `o`: open the node at the cursor with its run config. `App` only builds
//! the `RunRequest`; the loop runs it (`Flow::Run`).

use std::process::ExitStatus;

use crate::{
    model::{settings::RunName, tree::Tree},
    run::{Event, Library, RunContext, RunError, exit_code},
    tui::app::{App, Flow, RunRequest},
};

impl App<'_> {
    /// `o`: the node at the cursor with its `open_with`. Every problem is a
    /// toast, and the TUI stays (no flicker for a typo in a setting).
    pub(super) fn open(&mut self) -> Flow {
        match run_request(self.core.tree(), &self.tree_state.cursor) {
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

/// What `o` on the node at `path` runs. A task only for now; a container
/// needs the task picker (4b).
fn run_request(tree: &Tree, path: &[usize]) -> Result<RunRequest, String> {
    let node = tree.get(path).ok_or("no such node")?;
    if node.as_task().is_none() {
        return Err(format!(
            "{} is no task: opening containers comes with the picker",
            node.name()
        ));
    }
    let name = tree
        .open_with(path)
        .ok_or_else(|| format!("no run config for {} (set open_with)", node.name()))?;
    let library = Library::load(&tree.run_dir()).map_err(|e| e.to_string())?;
    let script = library
        .find(&name)
        .map_err(|e| e.to_string())?
        .to_path_buf();
    let ctx = RunContext::new(tree, Event::Open, path, Some(path)).ok_or("no such node")?;
    Ok(RunRequest { name, script, ctx })
}
