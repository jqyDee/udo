//! TUI state + all key handling. No terminal I/O here, so everything is
//! testable: feed keys into `handle_key`, check tree / mode / toast.
//!
//! One file per mode that needs more than a line or two:
//! - `confirm`: "remove?" prompt (`d`)
//! - `create`:  new task / container forms (`t` `T` `c` `C`)

mod confirm;
mod create;

use std::time::Instant;

use crossterm::event::{KeyEvent, KeyEventKind};
use ratatui::widgets::ListState;

pub use confirm::Confirm;

use crate::{
    model::{task::TaskStatus, tree::Tree},
    tui::{
        form::Form,
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
    /// "Remove?" prompt open; only y / n / esc do anything.
    Confirm(Confirm),
    /// Create form open; keys go to `Form::handle_key`.
    Form(Box<Form>),
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
            Action::Up => self.tree.move_up(),
            Action::Down => self.tree.move_down(),
            Action::In => self.tree.move_in(),
            Action::Out => self.tree.move_out(),
            Action::Toggle => self.tree.toggle_collapse(),
            Action::CollapseAll => self.tree.collapse_all(),
            Action::ExpandAll => self.tree.expand_all(),
            Action::SetStatus(status) => self.set_status(status).await,
            Action::Delete => self.ask_delete(),
            Action::NewContainer { global } => self.open_container_form(global),
            Action::NewTask { global } => self.open_task_form(global),
        }
        Flow::Continue
    }

    async fn set_status(&mut self, status: TaskStatus) {
        let path = self.tree.cursor.clone();
        match self.tree.set_task_status(&path, status).await {
            Ok(()) => {
                let name = self.tree.get(&path).map_or("", |n| n.name());
                self.info(format!("{name} -> {status}"));
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

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyCode, KeyEventState};

    use super::*;
    use crate::{
        model::node::Node,
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
        let path = t.create(&[], task("sheet")).await.unwrap();
        t.cursor = path.clone();
        let mut app = App::new(&mut t);

        app.handle_key(key('x')).await;

        let toast = app.toast.as_ref().expect("no toast");
        assert_eq!(toast.kind, ToastKind::Info);
        assert_eq!(toast.msg, "sheet -> done");
        let sheet = app.tree.get(&path).and_then(Node::as_task);
        assert_eq!(sheet.expect("expected a task").status, TaskStatus::Finished);
    }

    // ---------- delete + confirm ----------
    // The in-memory `tree()` is never saved: only answers that don't delete
    // (n, esc, ignored keys) use it. `y` saves, so it gets a real tempdir tree.

    #[tokio::test]
    async fn d_opens_confirm_for_selected_node() {
        let mut t = tree(&[0]);
        let mut app = App::new(&mut t);

        assert_eq!(app.handle_key(key('d')).await, Flow::Continue);

        assert_eq!(
            app.mode,
            Mode::Confirm(Confirm {
                path: vec![0],
                name: "a".into()
            })
        );
    }

    #[tokio::test]
    async fn n_and_esc_cancel_without_deleting() {
        for cancel in [key('n'), press(KeyCode::Esc)] {
            let mut t = tree(&[0]);
            let mut app = App::new(&mut t);
            app.handle_key(key('d')).await;

            app.handle_key(cancel).await;

            assert_eq!(app.mode, Mode::Normal, "{cancel:?} did not close");
            assert_eq!(app.tree.get(&[0]).unwrap().name(), "a");
            assert!(app.toast.is_none());
        }
    }

    #[tokio::test]
    async fn other_keys_are_ignored_while_confirming() {
        let mut t = tree(&[0]);
        let mut app = App::new(&mut t);
        app.handle_key(key('d')).await;

        // neither moves the cursor behind the prompt nor quits
        assert_eq!(app.handle_key(key('j')).await, Flow::Continue);
        assert_eq!(app.handle_key(key('q')).await, Flow::Continue);

        assert!(matches!(app.mode, Mode::Confirm(_)));
        assert_eq!(app.tree.cursor, vec![0]);
    }

    #[tokio::test]
    async fn d_without_selection_shows_error() {
        let mut t = tree_with(vec![], &[]); // empty tree: cursor stays on the root
        let mut app = App::new(&mut t);

        app.handle_key(key('d')).await;

        assert_eq!(app.mode, Mode::Normal);
        let toast = app.toast.as_ref().expect("no toast");
        assert_eq!(toast.kind, ToastKind::Error);
    }

    #[tokio::test]
    async fn y_removes_node_from_tree_and_disk() {
        let tmp = tempfile::tempdir().unwrap();
        let mut t = Tree::load_from(tmp.path()).await.unwrap(); // real root: saving works
        let path = t.create(&[], task("sheet")).await.unwrap();
        t.cursor = path.clone();
        let mut app = App::new(&mut t);

        app.handle_key(key('d')).await;
        app.handle_key(key('y')).await;

        assert_eq!(app.mode, Mode::Normal);
        assert!(app.tree.get(&path).is_none());
        assert!(app.tree.cursor.is_empty()); // last node gone: nothing selected
        let toast = app.toast.as_ref().expect("no toast");
        assert_eq!(toast.kind, ToastKind::Info);
        assert!(toast.msg.contains("removed sheet"), "got: {}", toast.msg);

        // saved: a fresh load doesn't have it either
        let reloaded = Tree::load_from(tmp.path()).await.unwrap();
        assert!(reloaded.rows().is_empty());
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

    // ---------- create forms ----------

    fn type_str(s: &str) -> Vec<KeyEvent> {
        s.chars().map(key).collect()
    }

    #[tokio::test]
    async fn t_type_enter_creates_task_and_selects_it() {
        let tmp = tempfile::tempdir().unwrap();
        let mut t = Tree::load_from(tmp.path()).await.unwrap(); // empty root
        let mut app = App::new(&mut t);

        app.handle_key(key('t')).await;
        assert!(matches!(app.mode, Mode::Form(_)));
        for k in type_str("exam") {
            app.handle_key(k).await;
        }
        app.handle_key(press(KeyCode::Enter)).await;

        assert_eq!(app.mode, Mode::Normal);
        assert_eq!(app.toast.as_ref().unwrap().kind, ToastKind::Info);
        assert_eq!(app.tree.cursor, vec![0]);
        assert_eq!(app.tree.get(&[0]).unwrap().name(), "exam");
        assert_eq!(app.tree.get(&[0]).unwrap().header.description, None); // left empty
    }

    #[tokio::test]
    async fn description_from_form_is_trimmed_and_saved() {
        let tmp = tempfile::tempdir().unwrap();
        let mut t = Tree::load_from(tmp.path()).await.unwrap(); // empty root
        let mut app = App::new(&mut t);

        app.handle_key(key('t')).await;
        for k in type_str("exam") {
            app.handle_key(k).await;
        }
        app.handle_key(press(KeyCode::Tab)).await; // -> description
        for k in type_str("  read ch 3 ") {
            app.handle_key(k).await;
        }
        app.handle_key(press(KeyCode::Enter)).await;

        let desc = &app.tree.get(&[0]).unwrap().header.description;
        assert_eq!(desc.as_deref(), Some("read ch 3"));
    }

    #[tokio::test]
    async fn invalid_name_keeps_form_open_with_error() {
        let tmp = tempfile::tempdir().unwrap();
        let mut t = Tree::load_from(tmp.path()).await.unwrap();
        let mut app = App::new(&mut t);

        app.handle_key(key('t')).await;
        for k in type_str("a/b") {
            app.handle_key(k).await;
        }
        app.handle_key(press(KeyCode::Enter)).await;

        assert!(matches!(app.mode, Mode::Form(_)), "form closed on error");
        assert_eq!(app.toast.as_ref().unwrap().kind, ToastKind::Error);
        assert!(app.tree.get(&[]).unwrap().children().is_empty());
    }

    #[tokio::test]
    async fn esc_closes_form_without_creating() {
        let mut t = tree(&[0]);
        let mut app = App::new(&mut t);

        app.handle_key(key('c')).await;
        app.handle_key(key('x')).await;
        app.handle_key(press(KeyCode::Esc)).await;

        assert_eq!(app.mode, Mode::Normal);
        assert_eq!(app.tree.get(&[]).unwrap().children().len(), 2);
    }
}
