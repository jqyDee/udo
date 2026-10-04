//! TUI state + all key handling. No terminal I/O here, so everything is
//! testable: feed keys into `handle_key`, check tree / mode / toast.
//!
//! `Mode` (in `mode`) says what the keys do; help (`?`) wraps the mode it
//! was opened from. One file per mode that needs more than a line or two:
//! - `confirm`: generic yes / no prompt (`Confirm`, its `ConfirmAction`)
//! - `details`: `DetailsTab`, which tab the right pane shows (Tab / Shift+Tab)
//! - `forms`:   create forms (`t` `T` `c` `C`), the edit form (`e`) and the
//!   settings form (`e` on the settings tab)
//! - `open`:    open the node at the cursor with its run config (`o`) or a
//!   picked one (`O`), a container via the task picker: `Flow::Run` for
//!   the loop, `after_run` when it is back
//! - `pick`:    keys while a picker is open (`Mode::Pick`), and what a pick
//!   leads to
//! - `remove`:  "remove?" prompt (`d`) and full delete (`D`) of a node
//! - `sessions`: the cursor in the sessions tab's list (`e` on the tab, `esc`),
//!   adding (`a`), editing (`e`), splitting (`s`), cutting (`c`) and
//!   removing (`d`) its sessions
//! - `timer`:   start / stop the timer on the task at the cursor (`s`)

mod confirm;
pub mod details;
mod forms;
mod mode;
mod open;
mod pick;
mod remove;
mod sessions;
#[cfg(test)]
mod tests;
mod timer;

use std::{collections::HashSet, time::Instant};

use crossterm::event::{KeyEvent, KeyEventKind};

pub use confirm::{Confirm, ConfirmAction, ConfirmStage};
pub use mode::Mode;
pub use remove::PurgeOption;

/// Lives in `run` (the CLI builds them too); here for `Flow::Run`.
pub use crate::run::RunRequest;

use crate::{
    core::Core,
    estimate::History,
    model::{
        id::NodeId,
        node::Node,
        sessions::Session,
        time::{self, Clock},
        tree::{TrashFn, system_trash},
    },
    tui::{
        app::details::DetailsTab,
        keys::{self, Action},
        session_list::SessionList,
        toast::Toast,
        tree_state::TreeState,
    },
};

/// What the event loop should do after a key.
#[derive(Debug, Clone, PartialEq)]
pub enum Flow {
    Continue,
    Quit,
    /// Hand the terminal to a script: the loop leaves the TUI, runs it,
    /// comes back and calls `App::after_run`. `App` only says what to run;
    /// the terminal is the loop's. Boxed: every key returns a `Flow`, and
    /// `Continue` should not carry a request's size.
    Run(Box<RunRequest>),
}

pub struct App<'a> {
    /// Where "now" comes from for session corrections (`time::now`); tests set
    /// a fixed time, the future check depends on it.
    pub clock: Clock,
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
    /// Tasks with sessions ("started" when not done), reloaded after every
    /// key (`reload`).
    pub with_sessions: HashSet<NodeId>,
    /// The running session, if any; reloaded after every key and on every
    /// tick (`reload`).
    pub running: Option<Session>,
    /// Sessions of the node at the cursor (a container: of every task below
    /// it), oldest first; for the details time rows and the sessions tab.
    /// Reloaded with the rest.
    pub sessions: Vec<Session>,
    /// Page of the sessions tab; `page_len` is set by every draw.
    pub session_list: SessionList,
    /// What the estimates are computed from; reloaded with the rest
    /// (`reload`), so moving the cursor costs nothing.
    pub history: History,
}

impl<'a> App<'a> {
    /// Starts on `tree_state`'s cursor (`[]` = the root row).
    /// `TreeState::load` picks where a fresh start begins. Call `reload`
    /// before the first draw.
    pub fn new(core: &'a mut Core, tree_state: TreeState) -> Self {
        Self {
            clock: time::now,
            core,
            tree_state,
            mode: Mode::default(),
            details_tab: DetailsTab::default(),
            toast: None,
            trash: system_trash,
            with_sessions: HashSet::new(),
            running: None,
            sessions: Vec::new(),
            session_list: SessionList::default(),
            history: History::default(),
        }
    }

    /// Read again what the views need from the stores. A failed read keeps
    /// the old values and says so.
    pub async fn reload(&mut self) {
        let old = self.newest_ids();
        match self.core.sessions_of(&self.tree_state.cursor).await {
            Ok(sessions) => self.sessions = sessions,
            Err(e) => self.error(e.to_string()),
        }
        self.follow_sessions(&old);
        match self.core.tasks_with_sessions().await {
            Ok(with_sessions) => self.with_sessions = with_sessions,
            Err(e) => self.error(e.to_string()),
        };
        match self.core.running_session().await {
            Ok(running) => self.running = running,
            Err(e) => self.error(e.to_string()),
        }
        match self.core.history((self.clock)()).await {
            Ok(history) => self.history = history,
            Err(e) => self.error(e.to_string()),
        }
    }

    /// Handle one key according to the current mode, then `reload`.
    pub async fn handle_key(&mut self, key: KeyEvent) -> Flow {
        if key.kind != KeyEventKind::Press {
            return Flow::Continue;
        }
        let flow = self.dispatch(key).await;
        self.reload().await;
        flow
    }

    async fn dispatch(&mut self, key: KeyEvent) -> Flow {
        match self.mode {
            Mode::Help(_) => {
                self.close_help();
                Flow::Continue
            }
            Mode::Normal => match self.action_for(key) {
                Some(action) => self.run(action).await,
                None => Flow::Continue,
            },
            Mode::Sessions => self.handle_list_key(key).await,
            Mode::Confirm(_) => self.answer_confirm(key).await,
            Mode::Form(_) => self.handle_form_key(key).await,
            Mode::Pick(_) => self.handle_pick_key(key),
        }
    }

    /// Run one action. Results and errors end up as a toast, never as `Err`:
    /// a failed action must not end the TUI.
    async fn run(&mut self, action: Action) -> Flow {
        match action {
            Action::Quit => return Flow::Quit,
            Action::Help => self.open_help(),
            Action::Up => self.tree_state.move_up(self.core.tree()),
            Action::Down => self.tree_state.move_down(self.core.tree()),
            Action::In => self.tree_state.move_in(self.core.tree()),
            Action::Out => self.tree_state.move_out(),
            Action::Toggle => self.tree_state.toggle_collapse(self.core.tree()),
            Action::CollapseAll => self.tree_state.collapse_all(self.core.tree()),
            Action::ExpandAll => self.tree_state.expand_all(),
            Action::ToggleDone => self.toggle_done().await,
            Action::Edit => match self.details_tab {
                DetailsTab::Info => self.open_edit_form(),
                DetailsTab::Settings => self.open_settings_form(),
                DetailsTab::Sessions => self.enter_list(),
            },
            Action::Delete => self.ask_delete(),
            Action::ToggleTimer => self.toggle_timer().await,
            Action::NewContainer => self.open_container_form(),
            Action::NewTask => self.open_task_form(),
            Action::NextTab => self.details_tab.next(),
            Action::PrevTab => self.details_tab.prev(),
            Action::Open => return self.open(),
            Action::OpenWith => return self.pick_script(),
            // only bound in `SESSION_LIST_KEYMAP` / `PICKER_KEYMAP`
            Action::Back | Action::Add | Action::Split | Action::Cut | Action::Pick => {}
        }
        Flow::Continue
    }

    /// Done -> reopened (to do / started), else done. Not a task: error.
    async fn toggle_done(&mut self) {
        let path = self.tree_state.cursor.clone();
        let done = self
            .core
            .tree()
            .get(&path)
            .and_then(Node::as_task)
            .is_some_and(|t| t.done_at.is_some());
        match self.core.set_done(&path, !done, time::now()).await {
            Ok(stopped) => {
                // set_done succeeded: a task
                let node = self.core.tree().get(&path);
                let status = node
                    .and_then(|n| Some((n, n.as_task()?)))
                    .map(|(n, t)| t.status(self.with_sessions.contains(&n.id())).to_string());
                let name = node.map_or("", |n| n.name());
                let status = status.unwrap_or_default();
                self.info(format!(
                    "{name} -> {status}{}",
                    timer_note(stopped.is_some())
                ));
            }
            Err(e) => self.error(e.to_string()),
        }
    }

    /// `?` (tree or sessions list): key help over the current mode; any
    /// key goes back to it (`close_help`).
    fn open_help(&mut self) {
        let under = std::mem::take(&mut self.mode);
        self.mode = Mode::Help(Box::new(under));
    }

    /// Any key while help is open: back to the mode it was opened from,
    /// exactly as it was. The key does nothing else.
    fn close_help(&mut self) {
        let Mode::Help(under) = std::mem::take(&mut self.mode) else {
            unreachable!("only called while Mode::Help");
        };
        self.mode = *under;
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

    /// See `Mode::in_list`.
    pub fn in_list(&self) -> bool {
        self.mode.in_list()
    }

    /// The action `key` stands for in the current mode's keymap (None: no
    /// keymap, or the key isn't in it). Dispatch and `?` help use the same
    /// `Mode::keymap`, so the help shows exactly what the keys do.
    fn action_for(&self, key: KeyEvent) -> Option<Action> {
        self.mode
            .keymap()
            .and_then(|keys| keys::action_for_in(keys, key))
    }
}

/// Toast suffix when an action also stopped the timer (done, delete).
fn timer_note(stopped: bool) -> &'static str {
    if stopped { ", timer stopped" } else { "" }
}
