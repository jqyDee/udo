//! "Remove?" prompt: `d` opens it, y / n / esc answer it. `D` switches to
//! the full delete (node and folders to the Trash), confirmed by typing the
//! folder path.

use crossterm::event::{KeyCode, KeyEvent};

use crate::{
    model::{NodePath, tree::PurgePlan},
    tui::form::TextInput,
};

use super::{App, Flow, Mode};

/// What a pending "remove?" prompt is about. Stored when `d` is pressed, so
/// the answer always applies to the node that was selected at that moment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Confirm {
    pub path: NodePath,
    pub name: String,
    /// Computed once when `d` is pressed.
    pub purge: PurgeOption,
    pub stage: ConfirmStage,
}

/// Whether `D` (full delete) is possible for the node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PurgeOption {
    /// `purge_plan` -> Ok(None): no folder, `D` not offered.
    NoFolder,
    /// `purge_plan` -> Err: the reason, shown dimmed in the popup.
    Refused(String),
    /// Boxed: a plan is big, and `Mode` holds this inline.
    Ready(Box<PurgePlan>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfirmStage {
    /// y / n / esc, plus D.
    Ask,
    /// Full delete: warning + path input.
    Purge { input: TextInput },
}

impl App<'_> {
    /// Open the confirm prompt for the selected node, with its full delete
    /// plan. Nothing selected (the root, e.g. empty tree) -> error toast.
    pub(super) fn ask_delete(&mut self) {
        let Some(node) = self.tree_state.selected(self.tree) else {
            return self.error("nothing selected");
        };
        let name = node.name().to_string();
        let path = self.tree_state.cursor.clone();
        let purge = match self.tree.purge_plan(&path) {
            Ok(Some(plan)) => PurgeOption::Ready(Box::new(plan)),
            Ok(None) => PurgeOption::NoFolder,
            Err(e) => PurgeOption::Refused(e.to_string()),
        };
        self.mode = Mode::Confirm(Confirm {
            path,
            name,
            purge,
            stage: ConfirmStage::Ask,
        });
    }

    /// A key while the prompt is open, by stage.
    pub(super) async fn answer_confirm(&mut self, key: KeyEvent) -> Flow {
        let purging = matches!(
            &self.mode,
            Mode::Confirm(Confirm {
                stage: ConfirmStage::Purge { .. },
                ..
            })
        );
        if purging {
            self.answer_purge(key).await;
        } else {
            self.answer_ask(key).await;
        }
        Flow::Continue
    }

    /// Ask stage: `y` removes the node (unregister only, files stay), `D`
    /// goes on to the full delete, `n`/esc cancel, anything else is ignored.
    async fn answer_ask(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('y') => {
                let Some(confirm) = self.close_confirm() else {
                    return;
                };
                match self.tree.delete(&confirm.path).await {
                    Ok(()) => {
                        self.tree_state.after_remove(self.tree, &confirm.path);
                        self.info(format!("removed {} (files kept)", confirm.name));
                    }
                    Err(e) => self.error(e.to_string()),
                }
            }
            KeyCode::Char('D') => self.start_purge(),
            KeyCode::Char('n') | KeyCode::Esc => self.mode = Mode::Normal,
            _ => {}
        }
    }

    /// `D`: on to the path input if a plan is ready, else a toast saying why
    /// not (the prompt stays open, `y` still works).
    fn start_purge(&mut self) {
        let Mode::Confirm(confirm) = &mut self.mode else {
            return;
        };
        let refused = match &confirm.purge {
            PurgeOption::Ready(_) => {
                confirm.stage = ConfirmStage::Purge {
                    input: TextInput::new(""),
                };
                return;
            }
            PurgeOption::NoFolder => None,
            PurgeOption::Refused(reason) => Some(reason.clone()),
        };
        match refused {
            Some(reason) => self.error(reason),
            None => self.info("no folder to delete, use y"),
        }
    }

    /// Purge stage: every key edits the path; `Enter` with exactly the
    /// shown path runs the full delete, other text keeps the popup open;
    /// esc closes it.
    async fn answer_purge(&mut self, key: KeyEvent) {
        let Mode::Confirm(confirm) = &mut self.mode else {
            return;
        };
        let (ConfirmStage::Purge { input }, PurgeOption::Ready(plan)) =
            (&mut confirm.stage, &confirm.purge)
        else {
            return;
        };
        match key.code {
            KeyCode::Esc => self.mode = Mode::Normal,
            KeyCode::Enter if input.value != plan.dir.display().to_string() => {
                self.error("path does not match");
            }
            KeyCode::Enter => {
                let plan = plan.clone();
                self.mode = Mode::Normal;
                self.run_purge(&plan).await;
            }
            _ => input.handle_key(key),
        }
    }

    /// Execute `plan` and report the result as a toast.
    async fn run_purge(&mut self, plan: &PurgePlan) {
        let report = match self.tree.purge(plan, self.trash).await {
            Ok(report) => report,
            Err(e) => return self.error(e.to_string()),
        };
        self.tree_state.after_remove(self.tree, &plan.path);
        match report.failed.split_first() {
            None => {
                let n = report.trashed.len();
                let folders = if n == 1 { "folder" } else { "folders" };
                self.info(format!(
                    "deleted {} · {n} {folders} moved to Trash",
                    plan.name
                ));
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

    /// Close the prompt, returning what it was about.
    fn close_confirm(&mut self) -> Option<Confirm> {
        match std::mem::replace(&mut self.mode, Mode::Normal) {
            Mode::Confirm(c) => Some(c),
            other => {
                self.mode = other;
                None
            }
        }
    }
}
