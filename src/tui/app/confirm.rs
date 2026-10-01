//! Generic yes / no prompt, the same shape as `Form` + `FormAction`: the
//! prompt knows only its keys (by `ConfirmStage`), the `ConfirmAction` says
//! what yes does. The node specifics (`d` / `D`) are in `remove`.

use crossterm::event::{KeyCode, KeyEvent};

use crate::{
    model::{NodePath, tree::PurgePlan},
    tui::form::TextInput,
};

use super::{App, Flow, Mode, PurgeOption};

/// Yes / no prompt. Knows only its keys; `action` says what yes does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Confirm {
    /// Popup texts, filled once when opening; drawn as they are.
    pub title: &'static str,
    pub question: String,
    /// Dim lines below the question.
    pub notes: Vec<String>,
    /// Key hints: ("y", "remove from udo"), ("D", "delete with files").
    pub keys: Vec<(&'static str, &'static str)>,
    /// `TypeToConfirm` only ("" otherwise): the line above the input, and
    /// the error toast when Enter is pressed with other text.
    pub type_prompt: &'static str,
    pub mismatch: &'static str,
    pub action: ConfirmAction,
    pub stage: ConfirmStage,
}

/// What yes does. Stored when the prompt opens, so the answer always
/// applies to what was selected at that moment. Every action has exactly
/// one "yes".
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfirmAction {
    /// `y`: unregister, files stay. `D` turns it into `PurgeNode` when
    /// `purge` is `Ready`.
    RemoveNode {
        path: NodePath,
        name: String,
        purge: PurgeOption,
    },
    /// The folder path typed: node and folders to the Trash.
    PurgeNode { plan: Box<PurgePlan> },
}

/// Which keys the prompt takes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfirmStage {
    /// y / N / esc (Enter = no), other keys go to the action.
    Ask,
    /// "Type X to confirm": Enter only counts once the text matches.
    TypeToConfirm { expected: String, input: TextInput },
}

impl App<'_> {
    /// A key while the prompt is open, by stage. Nothing in here knows the
    /// action: yes goes to `confirm_yes`, other keys of the Ask stage to
    /// `on_other_key`.
    pub(super) async fn answer_confirm(&mut self, key: KeyEvent) -> Flow {
        let Mode::Confirm(confirm) = &mut self.mode else {
            return Flow::Continue;
        };
        match &mut confirm.stage {
            ConfirmStage::Ask => match key.code {
                KeyCode::Char('y') => self.yes().await,
                // Enter = no: the capital in `y/N`
                KeyCode::Char('n') | KeyCode::Esc | KeyCode::Enter => {
                    self.mode = mode_after(&confirm.action);
                }
                _ => self.on_other_key(key),
            },
            ConfirmStage::TypeToConfirm { expected, input } => match key.code {
                KeyCode::Esc => self.mode = mode_after(&confirm.action),
                KeyCode::Enter if input.value != *expected => {
                    let mismatch = confirm.mismatch;
                    self.error(mismatch);
                }
                KeyCode::Enter => self.yes().await,
                _ => {
                    input.handle_key(key); // changed or not: nothing reacts to it
                }
            },
        }
        Flow::Continue
    }

    /// Yes: take the prompt out of the mode, then run its action.
    async fn yes(&mut self) {
        let Mode::Confirm(confirm) = std::mem::take(&mut self.mode) else {
            unreachable!("only called while Mode::Confirm");
        };
        self.mode = mode_after(&confirm.action);
        self.confirm_yes(confirm.action).await;
    }

    /// What yes does, by action. Results and errors end up as a toast.
    async fn confirm_yes(&mut self, action: ConfirmAction) {
        match action {
            ConfirmAction::RemoveNode { path, name, .. } => self.remove_node(&path, &name).await,
            ConfirmAction::PurgeNode { plan } => self.run_purge(&plan).await,
        }
    }

    /// A key the Ask stage doesn't know: only `D` on `RemoveNode` does
    /// anything (on to the full delete), the rest is ignored.
    fn on_other_key(&mut self, key: KeyEvent) {
        let Mode::Confirm(confirm) = &self.mode else {
            return;
        };
        if matches!(confirm.action, ConfirmAction::RemoveNode { .. })
            && key.code == KeyCode::Char('D')
        {
            self.start_purge();
        }
    }
}

/// Where the keys go when the prompt for `action` closes (yes or no): the
/// tree for node actions. Also tells `App::in_list` where an open prompt
/// belongs.
pub(super) fn mode_after(action: &ConfirmAction) -> Mode {
    match action {
        ConfirmAction::RemoveNode { .. } | ConfirmAction::PurgeNode { .. } => Mode::Normal,
    }
}
