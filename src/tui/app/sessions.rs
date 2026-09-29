//! The cursor in the sessions tab's list: enter (`e` on the tab), move
//! (`j` `k` across pages, `h` `l` page by page), leave (`esc`), and stay on
//! its session across reloads. The rules live in `SessionList`; this file
//! maps keys and reloads to them.

use crossterm::event::KeyEvent;

use crate::{
    model::sessions::SessionId,
    tui::{
        app::{App, Flow, Mode},
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
            Some(Action::Down) => self.session_list.move_by(&ids, 1),
            Some(Action::Up) => self.session_list.move_by(&ids, -1),
            Some(Action::In) => self.session_list.turn_page(&ids, 1),
            Some(Action::Out) => self.session_list.turn_page(&ids, -1),
            Some(Action::Back) => self.leave_list(),
            _ => {} // keys of the tree do nothing here
        }
        Flow::Continue
    }

    /// Ids of `sessions` in display order (newest first).
    pub(super) fn newest_ids(&self) -> Vec<SessionId> {
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

    /// `esc`: the cursor back to the tree, nothing selected.
    fn leave_list(&mut self) {
        self.session_list.selected = None;
        self.mode = Mode::Normal;
    }
}
