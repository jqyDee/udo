//! TUI state + all key handling. No terminal I/O here, so everything is
//! testable: feed keys into `handle_key`, check tree / mode / toast.
//!
//! One file per mode that needs more than a line or two:
//! - `confirm`: "remove?" prompt (`d`) and full delete (`D`)
//! - `details`: `DetailsTab`, which tab the right pane shows (Tab / Shift+Tab)
//! - `forms`:   create forms (`t` `T` `c` `C`), the edit form (`e`) and the
//!   settings form (`e` on the settings tab)

mod confirm;
pub mod details;
mod forms;
#[cfg(test)]
mod tests;

use std::time::Instant;

use crossterm::event::{KeyEvent, KeyEventKind};

pub use confirm::{Confirm, ConfirmStage, PurgeOption};

use crate::{
    core::Core,
    model::{
        task::TaskStatus,
        time,
        tree::{TrashFn, system_trash},
    },
    tui::{
        app::details::DetailsTab,
        form::Form,
        keys::{Action, action_for},
        toast::Toast,
        tree_state::TreeState,
    },
};

/// What the keys currently do. One mode at a time, so e.g. "help open and a
/// confirm prompt open" can't happen. New prompts = new variants here.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub enum Mode {
    /// Keys go through `KEYMAP`.
    #[default]
    Normal,
    /// Key help overlay open; any key closes it.
    Help,
    /// "Remove?" prompt open: y / n / D / esc, then the path input of a
    /// full delete.
    Confirm(Confirm),
    /// Create or edit form open; keys go to `Form::handle_key`.
    Form(Box<Form>),
}

/// What the event loop should do after a key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flow {
    Continue,
    Quit,
}

pub struct App<'a> {
    /// The tree and the stores; every change goes through it.
    pub core: &'a mut Core,
    /// Cursor, folding and scroll of the tree pane.
    pub tree_state: TreeState,
    /// Which tab the right pane shows (Tab / Shift+Tab).
    pub details_tab: DetailsTab,
    pub mode: Mode,
    pub toast: Option<Toast>,
    /// How a full delete moves folders away: `system_trash`; tests swap in
    /// a fake so they never touch the real Trash.
    pub trash: TrashFn,
}

impl<'a> App<'a> {
    /// Starts on `tree_state`'s cursor (`[]` = the root row).
    /// `TreeState::load` picks where a fresh start begins.
    pub fn new(core: &'a mut Core, tree_state: TreeState) -> Self {
        Self {
            core,
            tree_state,
            mode: Mode::default(),
            details_tab: DetailsTab::default(),
            toast: None,
            trash: system_trash,
        }
    }

    /// Handle one key according to the current mode.
    pub async fn handle_key(&mut self, key: KeyEvent) -> Flow {
        if key.kind != KeyEventKind::Press {
            return Flow::Continue;
        }
        match self.mode {
            Mode::Help => {
                self.mode = Mode::Normal; // any key closes the help
                Flow::Continue
            }
            Mode::Normal => match action_for(key) {
                Some(action) => self.run(action).await,
                None => Flow::Continue,
            },
            Mode::Confirm(_) => self.answer_confirm(key).await,
            Mode::Form(_) => self.handle_form_key(key).await,
        }
    }

    /// Run one action. Results and errors end up as a toast, never as `Err`:
    /// a failed action must not end the TUI.
    async fn run(&mut self, action: Action) -> Flow {
        match action {
            Action::Quit => return Flow::Quit,
            Action::Help => self.mode = Mode::Help,
            Action::Up => self.tree_state.move_up(self.core.tree()),
            Action::Down => self.tree_state.move_down(self.core.tree()),
            Action::In => self.tree_state.move_in(self.core.tree()),
            Action::Out => self.tree_state.move_out(),
            Action::Toggle => self.tree_state.toggle_collapse(self.core.tree()),
            Action::CollapseAll => self.tree_state.collapse_all(self.core.tree()),
            Action::ExpandAll => self.tree_state.expand_all(),
            Action::SetStatus(status) => self.set_status(status).await,
            Action::Edit => match self.details_tab {
                DetailsTab::Info => self.open_edit_form(),
                DetailsTab::Settings => self.open_settings_form(),
            },
            Action::Delete => self.ask_delete(),
            Action::NewContainer => self.open_container_form(),
            Action::NewTask => self.open_task_form(),
            Action::NextTab => self.details_tab.next(),
            Action::PrevTab => self.details_tab.prev(),
        }
        Flow::Continue
    }

    async fn set_status(&mut self, status: TaskStatus) {
        let path = self.tree_state.cursor.clone();
        match self.core.set_status(&path, status, time::now()).await {
            Ok(stopped) => {
                let name = self.core.tree().get(&path).map_or("", |n| n.name());
                self.info(format!("{name} -> {status}{}", timer_note(stopped.is_some())));
            }
            Err(e) => self.error(e.to_string()),
        }
    }

    // --------------- Toast ---------------

    fn info(&mut self, msg: impl Into<String>) {
        self.toast = Some(Toast::info(msg));
    }

    fn error(&mut self, msg: impl Into<String>) {
        self.toast = Some(Toast::error(msg));
    }

    /// When the loop has to wake up to hide the toast (None: no toast).
    pub fn toast_deadline(&self) -> Option<Instant> {
        self.toast.as_ref().map(|t| t.until)
    }

    /// Drop the toast if it has expired at `now`.
    pub fn expire_toast(&mut self, now: Instant) {
        if self.toast.as_ref().is_some_and(|t| t.is_expired(now)) {
            self.toast = None;
        }
    }
}

/// Toast suffix when an action also stopped the timer (done, delete).
fn timer_note(stopped: bool) -> &'static str {
    if stopped { ", timer stopped" } else { "" }
}
