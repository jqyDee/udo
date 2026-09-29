//! The cursor in the sessions tab's list: enter (`e` on the tab), move
//! (`j` `k` across pages, `h` `l` page by page), leave (`esc`), and stay on
//! its session across reloads. The rules live in `SessionList`; this file
//! maps keys and reloads to them. `e` in the list opens the session's edit
//! form; saving it (`submit_session_form`) returns to the list.

use chrono::Local;
use crossterm::event::KeyEvent;

use crate::{
    model::{
        sessions::{SessionId, SessionPatch},
        time::{Minutes, Time},
    },
    tui::{
        app::{App, Flow, Mode, forms::to_time},
        form::Form,
        keys::{Action, LIST_KEYMAP, action_for_in},
    },
};

impl App<'_> {
    /// `e` on the sessions tab: the cursor on the first row of the shown
    /// page. No sessions: nothing to select, stay in the tree.
    pub(super) fn enter_list(&mut self) {
        if self.sessions.is_empty() {
            return self.info("no sessions yet");
        }
        let ids = self.newest_ids();
        let start = self.session_list.range(ids.len()).start;
        self.session_list.select(&ids, start);
        self.mode = Mode::Sessions;
    }

    /// A key while the cursor is in the list. Keys not in `LIST_KEYMAP`
    /// (the tree's) do nothing here.
    pub(super) async fn handle_list_key(&mut self, key: KeyEvent) -> Flow {
        let ids = self.newest_ids();
        match action_for_in(LIST_KEYMAP, key) {
            Some(Action::Quit) => return Flow::Quit,
            Some(Action::Down) => self.session_list.move_by(&ids, 1),
            Some(Action::Up) => self.session_list.move_by(&ids, -1),
            Some(Action::In) => self.session_list.turn_page(&ids, 1),
            Some(Action::Out) => self.session_list.turn_page(&ids, -1),
            Some(Action::Edit) => self.open_session_form(),
            Some(Action::Back) => self.leave_list(),
            _ => {} // keys of the tree do nothing here
        }
        Flow::Continue
    }

    /// Ids of `sessions` in display order (newest first); also for drawing
    /// (`SessionList::set_page_len`).
    pub(crate) fn newest_ids(&self) -> Vec<SessionId> {
        self.sessions.iter().rev().map(|s| s.id).collect()
    }

    /// After `sessions` was reloaded (`old`: the ids before): keep the list
    /// on its session (`SessionList::follow`). The last session gone while
    /// in the list: back to the tree.
    pub(super) fn follow_sessions(&mut self, old: &[SessionId]) {
        let Some(node) = self
            .core
            .tree()
            .get(&self.tree_state.cursor)
            .map(|n| n.id())
        else {
            return; // a cursor pointing nowhere: nothing to follow
        };
        let new = self.newest_ids();
        let still_selected = self.session_list.follow(node, old, &new);
        if !still_selected && self.mode == Mode::Sessions {
            self.mode = Mode::Normal;
            self.info("no sessions left");
        }
    }

    /// `e` in the list: the edit form of the selected session. The list
    /// keeps its selection underneath, closing the form returns to it.
    fn open_session_form(&mut self) {
        let selected = self.session_list.selected;
        let Some(s) = self.sessions.iter().find(|s| Some(s.id) == selected) else {
            return; // nothing selected: the list always has one
        };
        self.mode = Mode::Form(Box::new(Form::edit_session(s)));
    }

    /// Save the session form: only changed times go into the patch (an
    /// unchanged form is no edit, so no `edited` marker). Success: back to the
    /// list, the cursor stays on the session. Error: toast, the form stays open.
    pub(super) async fn submit_session_form(&mut self, id: SessionId, form: &Form) {
        let Some(s) = self.sessions.iter().find(|s| s.id == id).cloned() else {
            self.mode = Mode::Sessions;
            return self.error("that session is gone (removed elsewhere?)");
        };
        let (start, end) = form.session_times();
        let times = to_time(start).and_then(|start| Ok((start, end.map(to_time).transpose()?)));
        let (start, end) = match times {
            Ok(times) => times,
            Err(e) => return self.error(e),
        };

        let patch = SessionPatch {
            start: (start != s.start).then_some(start),
            end: end.filter(|&e| Some(e) != s.end),
        };
        if patch.start.is_none() && patch.end.is_none() {
            self.mode = Mode::Sessions; // nothing changed: no edit
            return;
        }
        match self.core.edit_session(id, patch, (self.clock)()).await {
            Ok(()) => {
                self.mode = Mode::Sessions;
                self.info(saved_text(&s.task.name, start, end.or(s.end)));
            }
            Err(e) => self.error(e.to_string()), // the form stays open
        }
    }

    /// `esc`: the cursor back to the tree, nothing selected.
    fn leave_list(&mut self) {
        self.session_list.selected = None;
        self.mode = Mode::Normal;
    }
}

/// Toast after saving: `lab 3: 14:05–15:00 (55m)`, local like the rows;
/// running (`end` None): `lab 3: 14:05–now`.
fn saved_text(name: &str, start: Time, end: Option<Time>) -> String {
    let clock = |t: Time| t.with_timezone(&Local).format("%H:%M").to_string();
    match end {
        Some(end) => {
            let minutes = (end - start).num_minutes().max(0);
            let length = Minutes::new(u32::try_from(minutes).unwrap_or(u32::MAX));
            format!("{name}: {}–{} ({length})", clock(start), clock(end))
        }
        None => format!("{name}: {}–now", clock(start)),
    }
}
