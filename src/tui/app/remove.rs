//! Removing nodes: `d` asks (`Confirm::remove_node`), `y` unregisters,
//! `D` goes on to the full delete (`Confirm::purge_node`): node and
//! folders to the Trash, confirmed by typing the folder path.

use crate::{
    model::{NodePath, time, tree::PurgePlan},
    tui::form::TextInput,
};

use super::{App, Confirm, ConfirmAction, ConfirmStage, Mode, timer_note};

/// Whether `D` (full delete) is possible for the node. Computed once when
/// `d` is pressed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PurgeOption {
    /// `purge_plan` -> Ok(None): no folder, `D` not offered.
    NoFolder,
    /// `purge_plan` -> Err: the reason, shown dimmed in the popup.
    Refused(String),
    /// Boxed: a plan is big, and `ConfirmAction` holds this inline.
    Ready(Box<PurgePlan>),
}

impl Confirm {
    /// `d` on a node: "Remove … from udo?" (Ask stage); `D` is offered
    /// only when `purge` is `Ready`, a refusal is shown as a note.
    pub fn remove_node(path: NodePath, name: String, purge: PurgeOption) -> Self {
        let mut notes = vec!["Files and folders stay on disk.".to_string()];
        if let PurgeOption::Refused(reason) = &purge {
            notes.push(format!("full delete not possible: {reason}"));
        }
        let mut keys = vec![("y", "remove from udo")];
        if matches!(purge, PurgeOption::Ready(_)) {
            keys.push(("D", "delete with files"));
        }
        keys.push(("n/esc", "cancel"));
        Self {
            title: " remove? ",
            question: format!("Remove \"{name}\" from udo?"),
            notes,
            keys,
            type_prompt: "",
            mismatch: "",
            action: ConfirmAction::RemoveNode { path, name, purge },
            stage: ConfirmStage::Ask,
        }
    }

    /// `D` with a plan: the full delete, confirmed by typing the folder path.
    pub fn purge_node(plan: Box<PurgePlan>) -> Self {
        Self {
            title: " delete with files ",
            question: format!("delete {} and everything in it?", plan.name),
            notes: vec!["everything is moved to the Trash".into()],
            keys: vec![("enter", "delete"), ("esc", "cancel")],
            type_prompt: "type the folder path to confirm:",
            mismatch: "path does not match",
            // before `action`: that moves `plan`
            stage: ConfirmStage::TypeToConfirm {
                expected: plan.dir.display().to_string(),
                input: TextInput::new(""),
            },
            action: ConfirmAction::PurgeNode { plan },
        }
    }
}

impl App<'_> {
    /// Open the confirm prompt for the selected node, with its full delete
    /// plan. Nothing selected (the root, e.g. empty tree) -> error toast.
    pub(super) fn ask_delete(&mut self) {
        if self.tree_state.on_root() {
            return self.error("the root cannot be removed");
        }
        let Some(node) = self.tree_state.selected(self.core.tree()) else {
            return self.error("nothing selected");
        };
        let name = node.name().to_string();
        let path = self.tree_state.cursor.clone();
        let purge = match self.core.purge_plan(&path) {
            Ok(Some(plan)) => PurgeOption::Ready(Box::new(plan)),
            Ok(None) => PurgeOption::NoFolder,
            Err(e) => PurgeOption::Refused(e.to_string()),
        };
        self.mode = Mode::Confirm(Box::new(Confirm::remove_node(path, name, purge)));
    }

    /// `y` on `RemoveNode`: unregister the node, its files stay.
    pub(super) async fn remove_node(&mut self, path: &NodePath, name: &str) {
        match self.core.delete(path, time::now()).await {
            Ok(stopped) => {
                self.tree_state.after_remove(self.core.tree(), path);
                self.info(format!("removed {name} (files kept){}", timer_note(stopped.is_some())));
            }
            Err(e) => self.error(e.to_string()),
        }
    }

    /// `D` on `RemoveNode`: on to the path input if a plan is ready, else a
    /// toast saying why not (the prompt stays open, `y` still works).
    pub(super) fn start_purge(&mut self) {
        let Mode::Confirm(confirm) = &self.mode else {
            return;
        };
        let ConfirmAction::RemoveNode { purge, .. } = &confirm.action else {
            return;
        };
        match purge {
            PurgeOption::Ready(plan) => {
                self.mode = Mode::Confirm(Box::new(Confirm::purge_node(plan.clone())));
            }
            PurgeOption::NoFolder => self.info("no folder to delete, use y"),
            PurgeOption::Refused(reason) => self.error(reason.clone()),
        }
    }

    /// Enter with the folder path typed: execute `plan` and report the
    /// result as a toast.
    pub(super) async fn run_purge(&mut self, plan: &PurgePlan) {
        let (report, stopped) = match self.core.purge(plan, self.trash, time::now()).await {
            Ok(done) => done,
            Err(e) => return self.error(e.to_string()),
        };
        self.tree_state.after_remove(self.core.tree(), &plan.path);
        match report.failed.split_first() {
            None => {
                let n = report.trashed.len();
                let folders = if n == 1 { "folder" } else { "folders" };
                let note = timer_note(stopped.is_some());
                self.info(format!("deleted {} · {n} {folders} moved to Trash{note}", plan.name));
            }
            Some(((dir, reason), rest)) => {
                let mut msg = format!(
                    "deleted {}, but could not trash {}: {reason}",
                    plan.name,
                    dir.display()
                );
                if !rest.is_empty() {
                    msg.push_str(&format!(" and {} more", rest.len()));
                }
                self.error(msg);
            }
        }
    }
}
