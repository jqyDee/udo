//! The cursor in the sessions tab's list: enter (`e` on the tab), move
//! (`j` `k` across pages, `h` `l` page by page), key help (`?`), leave
//! (`esc`), and stay on its session across reloads. The rules live in `SessionList`; this file
//! maps keys and reloads to them. `e` in the list opens the session's edit
//! form; saving it (`save_session`) returns to the list, like the add
//! (`a`, `save_add_session`), split (`s`, `save_split`) and cut (`c`,
//! `save_cut`) forms. `d` asks before
//! removing the session (`Confirm::remove_session`); either answer returns
//! to the list.

use chrono::Local;
use crossterm::event::KeyEvent;

use crate::{
    Res,
    core::SPLIT_AT_EDGE,
    model::{
        NodePath,
        sessions::{Session, SessionId, SessionPatch},
        time::{Minutes, Time},
    },
    tui::{
        app::{App, Confirm, ConfirmAction, ConfirmStage, Flow, Mode, forms::Saved, timer_note},
        form::Form,
        keys::Action,
    },
};

impl App<'_> {
    /// `e` on the sessions tab: the cursor on the first row of the shown
    /// page. No sessions: the empty list, nothing selected.
    pub(super) fn enter_list(&mut self) {
        let ids = self.newest_ids();
        let start = self.session_list.range(ids.len()).start;
        self.session_list.select(&ids, start);
        self.mode = Mode::Sessions;
    }

    /// A key while the cursor is in the list. Keys not in `SESSION_LIST_KEYMAP`
    /// (the tree's) do nothing here.
    pub(super) async fn handle_list_key(&mut self, key: KeyEvent) -> Flow {
        let ids = self.newest_ids();
        match self.action_for(key) {
            Some(Action::Quit) => return Flow::Quit,
            Some(Action::Help) => self.open_help(),
            Some(Action::Down) => self.session_list.move_by(&ids, 1),
            Some(Action::Up) => self.session_list.move_by(&ids, -1),
            Some(Action::In) => self.session_list.turn_page(&ids, 1),
            Some(Action::Out) => self.session_list.turn_page(&ids, -1),
            Some(Action::Edit) => self.open_session_form(),
            Some(Action::Back) => self.leave_list(),
            Some(Action::Delete) => self.ask_remove_session(),
            Some(Action::Split) => self.open_split_form(),
            Some(Action::Cut) => self.open_cut_form(),
            Some(Action::Add) => self.open_add_form(),
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
    /// on its session (`SessionList::follow`). The last one gone: the empty
    /// list, the keys stay there.
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
        self.session_list.follow(node, old, &new);
    }

    /// The session under the list cursor; None (the list is empty): the
    /// toast `no session selected`.
    fn selected_session(&mut self) -> Option<Session> {
        let selected = self.session_list.selected;
        let found = self
            .sessions
            .iter()
            .find(|s| Some(s.id) == selected)
            .cloned();
        if found.is_none() {
            self.error("no session selected");
        }
        found
    }

    /// `a` in the list: add form for the task at the tree cursor (also in
    /// the empty list). A container's list: which task would it be on?
    fn open_add_form(&mut self) {
        let path = self.tree_state.cursor.clone();
        let Some(task) = self
            .core
            .tree()
            .get(&path)
            .filter(|n| n.as_task().is_some())
        else {
            return self.error("pick a task to add a session");
        };
        self.mode = Mode::Form(Box::new(Form::add_session(
            task.name(),
            path,
            (self.clock)(),
        )));
    }

    /// `e` in the list: the edit form of the selected session. The list
    /// keeps its selection underneath, closing the form returns to it.
    fn open_session_form(&mut self) {
        let Some(s) = self.selected_session() else {
            return;
        };
        self.mode = Mode::Form(Box::new(Form::edit_session(&s)));
    }

    /// `s` in the list: split form of the selected session; a running one
    /// can't be split (the store refuses it too, this says why first).
    fn open_split_form(&mut self) {
        let Some(s) = self.selected_session() else {
            return;
        };
        if s.end.is_none() {
            return self.error("stop the timer to split");
        }
        self.mode = Mode::Form(Box::new(Form::split_session(&s, (self.clock)())));
    }

    /// `c` in the list: cut form of the selected session (running too).
    fn open_cut_form(&mut self) {
        let Some(s) = self.selected_session() else {
            return;
        };
        self.mode = Mode::Form(Box::new(Form::cut_session(&s, (self.clock)())));
    }

    /// `d` in the list: ask before removing the selected session.
    fn ask_remove_session(&mut self) {
        let Some(s) = self.selected_session() else {
            return;
        };
        self.mode = Mode::Confirm(Box::new(Confirm::remove_session(&s)));
    }

    /// `y` on `RemoveSession`. The cursor goes to the neighbour on the
    /// next reload (`follow`), the keys back to the list (`mode_after`). A
    /// running session is stopped by the store first.
    pub(super) async fn remove_session(&mut self, id: SessionId) {
        let Some(s) = self.session_or_gone(id) else {
            return;
        };
        match self.core.delete_session(id).await {
            Ok(()) => self.info(format!(
                "removed {}{}",
                saved_text(&s.task.name, s.start, s.end),
                timer_note(s.end.is_none())
            )),
            Err(e) => self.error(e.to_string()),
        }
    }

    /// Add form: a new manual session on the task at `path`, selected.
    /// Future, end before start, overlap: Err, the form stays.
    pub(super) async fn save_add_session(&mut self, path: &NodePath, form: &Form) -> Res<Saved> {
        let (start, end) = form.add_times()?;
        let s = self
            .core
            .add_session(path, start, end, (self.clock)())
            .await?;
        Ok(Saved {
            reveal: None,
            msg: Some(format!(
                "added {}",
                saved_text(&s.task.name, start, Some(end))
            )),
            reveal_session: Some(s.id),
        })
    }

    /// Save the session form: only changed times go into the patch (an
    /// unchanged form is no edit, so no `edited` marker). Back to the list
    /// comes from `mode_after`; errors are shown by the caller.
    pub(super) async fn save_session(&mut self, id: SessionId, form: &Form) -> Res<Saved> {
        let Some(s) = self.session_or_gone(id) else {
            return Ok(Saved::nothing());
        };
        let (start, end) = form.session_times()?;

        let patch = SessionPatch {
            start: (start != s.start).then_some(start),
            end: end.filter(|&e| Some(e) != s.end),
        };
        if patch.start.is_none() && patch.end.is_none() {
            return Ok(Saved::nothing());
        }
        self.core.edit_session(id, patch, (self.clock)()).await?;
        Ok(Saved {
            reveal: None,
            msg: Some(saved_text(&s.task.name, start, end.or(s.end))),
            reveal_session: None,
        })
    }

    /// Split form: two sessions, the earlier half selected. At the start
    /// or end: Err, the form stays (the time can be fixed).
    pub(super) async fn save_split(&mut self, id: SessionId, form: &Form) -> Res<Saved> {
        let Some(s) = self.session_or_gone(id) else {
            return Ok(Saved::nothing());
        };
        let at = form.split_at()?;
        let Some((first, _)) = self.core.split_session(id, at).await? else {
            return Err(SPLIT_AT_EDGE.into());
        };
        Ok(Saved {
            reveal: None,
            msg: Some(format!("split {} at {}", s.task.name, clock(at))),
            reveal_session: Some(first.id),
        })
    }

    /// Cut form: `[from, to)` removed, the earliest piece left selected.
    pub(super) async fn save_cut(&mut self, id: SessionId, form: &Form) -> Res<Saved> {
        let Some(s) = self.session_or_gone(id) else {
            return Ok(Saved::nothing());
        };
        let (from, to) = form.cut_range()?;
        let left = self.core.cut_session(id, from, to, (self.clock)()).await?;
        Ok(Saved {
            reveal: None,
            msg: Some(format!(
                "cut {}–{} from {}",
                clock(from),
                clock(to),
                s.task.name
            )),
            reveal_session: left.first().map(|p| p.id),
        })
    }

    /// `esc`: the cursor back to the tree, nothing selected.
    fn leave_list(&mut self) {
        self.session_list.selected = None;
        self.mode = Mode::Normal;
    }

    /// The session `id` as loaded; gone (removed elsewhere): the toast,
    /// None. Its form / prompt then closes: nothing to fix there.
    fn session_or_gone(&mut self, id: SessionId) -> Option<Session> {
        let found = self.sessions.iter().find(|s| s.id == id).cloned();
        if found.is_none() {
            self.error("that session is gone (removed elsewhere?)");
        }
        found
    }
}

impl Confirm {
    /// `d` in the list: `remove session lab 3: 14:00–15:30 (1h30)?`; a
    /// running one says the timer stops.
    pub fn remove_session(s: &Session) -> Self {
        let running = if s.end.is_none() {
            " (running: the timer stops)"
        } else {
            ""
        };
        let what = saved_text(&s.task.name, s.start, s.end);
        Self {
            title: " remove? ",
            question: format!("remove session {what}?{running}"),
            notes: vec!["hidden from every list; the data stays".into()],
            keys: vec![("y", "remove"), ("n/esc", "cancel")],
            type_prompt: "",
            mismatch: "",
            action: ConfirmAction::RemoveSession { id: s.id },
            stage: ConfirmStage::Ask,
        }
    }
}

/// Toast after saving: `lab 3: 14:05–15:00 (55m)`, local like the rows;
/// running (`end` None): `lab 3: 14:05–now`.
fn saved_text(name: &str, start: Time, end: Option<Time>) -> String {
    match end {
        Some(end) => {
            let minutes = (end - start).num_minutes().max(0);
            let length = Minutes::new(u32::try_from(minutes).unwrap_or(u32::MAX));
            format!("{name}: {}–{} ({length})", clock(start), clock(end))
        }
        None => format!("{name}: {}–now", clock(start)),
    }
}

/// `14:05`: local, like the rows.
fn clock(t: Time) -> String {
    t.with_timezone(&Local).format("%H:%M").to_string()
}
