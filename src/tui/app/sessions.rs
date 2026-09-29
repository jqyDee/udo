//! The cursor in the sessions tab's list: enter (`e` on the tab), leave
//! (`esc`); moving and editing come with stage 3.

use crossterm::event::KeyEvent;

use crate::{
    model::sessions::Session,
    tui::{
        app::{App, Flow, Mode},
        keys::{Action, LIST_KEYMAP, action_for_in},
    },
};

impl App<'_> {
    /// `e` on the sessions tab: the cursor on the first row of the shown
    /// page. No sessions: nothing to select, stay in the tree.
    pub(super) fn enter_list(&mut self) {
        let start = self.session_list.range(self.sessions.len()).start;
        let Some(id) = self.newest_first(start).map(|s| s.id) else {
            return self.info("no sessions yet");
        };
        self.session_list.selected = Some(id);
        self.mode = Mode::Sessions;
    }

    /// A key while the cursor is in the list. Keys not in `LIST_KEYMAP`
    /// (the tree's) do nothing here.
    pub(super) async fn handle_list_key(&mut self, key: KeyEvent) -> Flow {
        // one action so far; a `match` again once `j` `k` `h` `l` `e` come
        if let Some(Action::Back) = action_for_in(LIST_KEYMAP, key) {
            self.leave_list();
        }
        Flow::Continue
    }

    fn leave_list(&mut self) {
        self.session_list.selected = None;
        self.mode = Mode::Normal;
    }

    /// The `i`-th session, newest first (as the tab shows them).
    fn newest_first(&self, i: usize) -> Option<&Session> {
        self.sessions.iter().rev().nth(i)
    }
}
