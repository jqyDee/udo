//! TUI state + all key handling. No terminal I/O here, so everything is
//! testable: feed keys into `handle_key`, check tree / mode / toast.

use std::time::Instant;

use crossterm::event::{KeyEvent, KeyEventKind};
use ratatui::widgets::ListState;

use crate::{
    model::{task::TaskStatus, tree::Tree},
    tui::{
        keys::{Action, action_for},
        toast::Toast,
    },
};

/// What the keys currently do. One mode at a time, so e.g. "help open and a
/// confirm prompt open" can't happen. New prompts = new variants here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mode {
    /// Keys go through `KEYMAP`.
    Normal,
    /// Key help overlay open; any key closes it.
    Help,
}

/// What the event loop should do after a key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flow {
    Continue,
    Quit,
}

pub struct App<'a> {
    pub tree: &'a mut Tree,
    pub mode: Mode,
    pub toast: Option<Toast>,
    /// Selection + scroll offset of the tree list (kept across frames).
    pub list: ListState,
}

impl<'a> App<'a> {
    /// Selects the first row if nothing is selected yet (fresh load).
    pub fn new(tree: &'a mut Tree) -> Self {
        if tree.cursor.is_empty() {
            tree.move_down();
        }
        Self {
            tree,
            mode: Mode::Normal,
            toast: None,
            list: ListState::default(),
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
        }
    }

    /// Run one action. Results and errors end up as a toast, never as `Err`:
    /// a failed action must not end the TUI.
    async fn run(&mut self, action: Action) -> Flow {
        match action {
            Action::Quit => return Flow::Quit,
            Action::Help => self.mode = Mode::Help,
            Action::Up => self.tree.move_up(),
            Action::Down => self.tree.move_down(),
            Action::In => self.tree.move_in(),
            Action::Out => self.tree.move_out(),
            Action::Toggle => self.tree.toggle_collapse(),
            Action::CollapseAll => self.tree.collapse_all(),
            Action::ExpandAll => self.tree.expand_all(),
            Action::SetStatus(status) => self.set_status(status).await,
        }
        Flow::Continue
    }

    async fn set_status(&mut self, status: TaskStatus) {
        let path = self.tree.cursor.clone();
        self.toast = Some(match self.tree.set_task_status(&path, status).await {
            Ok(()) => {
                let name = self.tree.get(&path).map_or("", |n| n.name());
                Toast::info(format!("{name} -> {status:?}"))
            }
            Err(e) => Toast::error(e.to_string()),
        });
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

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use crossterm::event::{KeyCode, KeyEventState};

    use super::*;
    use crate::{
        model::{node::Node, task::Task},
        test_util::{container, press, task, tree_with},
        tui::toast::ToastKind,
    };

    /// root: [a, ws: [b]], cursor on `cursor`. In memory, never saved.
    fn tree(cursor: &[usize]) -> Tree {
        tree_with(vec![task("a"), container("ws", vec![task("b")])], cursor)
    }

    fn key(c: char) -> KeyEvent {
        press(KeyCode::Char(c))
    }

    // ---------- flow + modes ----------

    #[tokio::test]
    async fn q_quits() {
        let mut t = tree(&[0]);
        let mut app = App::new(&mut t);
        assert_eq!(app.handle_key(key('q')).await, Flow::Quit);
    }

    #[tokio::test]
    async fn help_opens_and_any_key_closes_it_without_acting() {
        let mut t = tree(&[0]);
        let mut app = App::new(&mut t);

        app.handle_key(key('?')).await;
        assert_eq!(app.mode, Mode::Help);

        // `j` only closes the help, the cursor must not move
        assert_eq!(app.handle_key(key('j')).await, Flow::Continue);
        assert_eq!(app.mode, Mode::Normal);
        assert_eq!(app.tree.cursor, vec![0]);
    }

    #[tokio::test]
    async fn q_in_help_closes_help_instead_of_quitting() {
        let mut t = tree(&[0]);
        let mut app = App::new(&mut t);
        app.handle_key(key('?')).await;
        assert_eq!(app.handle_key(key('q')).await, Flow::Continue);
        assert_eq!(app.mode, Mode::Normal);
    }

    #[tokio::test]
    async fn key_release_is_ignored() {
        let mut t = tree(&[0]);
        let mut app = App::new(&mut t);
        let release = KeyEvent {
            kind: KeyEventKind::Release,
            state: KeyEventState::NONE,
            ..key('j')
        };
        app.handle_key(release).await;
        assert_eq!(app.tree.cursor, vec![0]);
    }

    #[tokio::test]
    async fn unknown_key_changes_nothing() {
        let mut t = tree(&[0]);
        let mut app = App::new(&mut t);
        assert_eq!(app.handle_key(key('#')).await, Flow::Continue);
        assert_eq!(app.tree.cursor, vec![0]);
        assert!(app.toast.is_none());
    }

    #[test]
    fn new_selects_first_row() {
        let mut t = tree(&[]);
        let app = App::new(&mut t);
        assert_eq!(app.tree.cursor, vec![0]);
    }

    // ---------- actions ----------

    #[tokio::test]
    async fn navigation_moves_cursor_without_toast() {
        let mut t = tree(&[0]);
        let mut app = App::new(&mut t);
        app.handle_key(key('j')).await;
        assert_eq!(app.tree.cursor, vec![1]);
        assert!(app.toast.is_none());
    }

    #[tokio::test]
    async fn set_status_on_container_shows_error_toast() {
        let mut t = tree(&[1]); // "ws": fails before any save
        let mut app = App::new(&mut t);

        assert_eq!(app.handle_key(key('x')).await, Flow::Continue);

        let toast = app.toast.as_ref().expect("no toast");
        assert_eq!(toast.kind, ToastKind::Error);
        assert!(toast.msg.contains("only tasks"), "got: {}", toast.msg);
    }

    #[tokio::test]
    async fn set_status_on_task_updates_and_shows_info_toast() {
        let tmp = tempfile::tempdir().unwrap();
        let mut t = Tree::load_from(tmp.path()).await.unwrap(); // real root: saving works
        let path = t
            .create_task(&[], Task::new("sheet".into(), None, Utc::now()))
            .await
            .unwrap();
        t.cursor = path.clone();
        let mut app = App::new(&mut t);

        app.handle_key(key('x')).await;

        let toast = app.toast.as_ref().expect("no toast");
        assert_eq!(toast.kind, ToastKind::Info);
        assert_eq!(toast.msg, "sheet -> Finished");
        let Some(Node::Task(sheet)) = app.tree.get(&path) else {
            panic!("expected a task at {path:?}");
        };
        assert_eq!(sheet.status, TaskStatus::Finished);
    }

    // ---------- toast lifetime ----------

    #[test]
    fn toast_deadline_and_expiry() {
        let mut t = tree(&[0]);
        let mut app = App::new(&mut t);
        assert_eq!(app.toast_deadline(), None);

        app.toast = Some(Toast::info("hi"));
        let until = app.toast_deadline().unwrap();

        app.expire_toast(until - std::time::Duration::from_millis(1));
        assert!(app.toast.is_some());
        app.expire_toast(until);
        assert!(app.toast.is_none());
    }
}
