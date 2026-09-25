//! Keys -> actions -> tree calls. Pure logic, no terminal.
//!
//! `KEYMAP` is the single source of truth: `action_for` looks keys up in it
//! and the help overlay is generated from it. New key = one line there.

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind};

use crate::{
    Res,
    model::{
        task::TaskStatus::{self, Finished, InProgress, Pending, Stale},
        tree::Tree,
    },
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Quit,
    Up,
    Down,
    In,
    Out,
    Toggle,
    CollapseAll,
    ExpandAll,
    SetStatus(TaskStatus),
    /// Toggle the key help overlay (UI only, handled by the event loop).
    Help,
}

/// One row of the keymap: all keys that trigger `action`, and its help text.
pub struct Binding {
    pub keys: &'static [KeyCode],
    pub action: Action,
    pub help: &'static str,
}

/// All key bindings. Order here = order in the help overlay.
#[rustfmt::skip] // keep one binding per line, like a table
pub const KEYMAP: &[Binding] = &[
    Binding { keys: &[KeyCode::Char('j'), KeyCode::Down], action: Action::Down, help: "move down" },
    Binding { keys: &[KeyCode::Char('k'), KeyCode::Up], action: Action::Up, help: "move up" },
    Binding { keys: &[KeyCode::Char('l'), KeyCode::Right], action: Action::In, help: "open / go in" },
    Binding { keys: &[KeyCode::Char('h'), KeyCode::Left], action: Action::Out, help: "go to parent" },
    Binding { keys: &[KeyCode::Char(' '), KeyCode::Enter], action: Action::Toggle, help: "fold / unfold" },
    Binding { keys: &[KeyCode::Char('z')], action: Action::CollapseAll, help: "fold all" },
    Binding { keys: &[KeyCode::Char('Z')], action: Action::ExpandAll, help: "unfold all" },
    Binding { keys: &[KeyCode::Char('x')], action: Action::SetStatus(Finished), help: "mark done" },
    Binding { keys: &[KeyCode::Char('p')], action: Action::SetStatus(InProgress), help: "mark in progress" },
    Binding { keys: &[KeyCode::Char('s')], action: Action::SetStatus(Stale), help: "mark stale" },
    Binding { keys: &[KeyCode::Char('u')], action: Action::SetStatus(Pending), help: "mark to do" },
    Binding { keys: &[KeyCode::Char('?')], action: Action::Help, help: "toggle this help" },
    Binding { keys: &[KeyCode::Char('q')], action: Action::Quit, help: "quit" },
];

/// Map a key press to an action via `KEYMAP`. Releases/repeats and unknown
/// keys -> None.
pub fn action_for(key: KeyEvent) -> Option<Action> {
    if key.kind != KeyEventKind::Press {
        return None;
    }
    KEYMAP
        .iter()
        .find(|b| b.keys.contains(&key.code))
        .map(|b| b.action)
}

/// Human-readable key name for the help overlay.
pub fn key_label(key: &KeyCode) -> String {
    match key {
        KeyCode::Char(' ') => "space".into(),
        KeyCode::Char(c) => c.to_string(),
        KeyCode::Up => "↑".into(),
        KeyCode::Down => "↓".into(),
        KeyCode::Left => "←".into(),
        KeyCode::Right => "→".into(),
        KeyCode::Enter => "enter".into(),
        KeyCode::Esc => "esc".into(),
        other => format!("{other:?}"),
    }
}

impl Action {
    /// Run the action on the tree. `Quit` and `Help` are handled by the event
    /// loop. Returns an optional message for the status line.
    pub async fn apply(self, tree: &mut Tree) -> Res<Option<String>> {
        match self {
            Action::Quit | Action::Help => {}
            Action::Up => tree.move_up(),
            Action::Down => tree.move_down(),
            Action::In => tree.move_in(),
            Action::Out => tree.move_out(),
            Action::Toggle => tree.toggle_collapse(),
            Action::CollapseAll => tree.collapse_all(),
            Action::ExpandAll => tree.expand_all(),
            Action::SetStatus(s) => {
                let path = tree.cursor.clone();
                tree.set_task_status(&path, s).await?;
                let name = tree.get(&path).map_or("", |n| n.name());
                return Ok(Some(format!("{name} -> {s:?}")));
            }
        }
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use chrono::Utc;
    use crossterm::event::{KeyEventState, KeyModifiers};

    use super::*;
    use crate::model::{
        container::{Container, ContainerKind},
        node::Node,
        task::Task,
    };

    fn press(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    /// In-memory tree: root: [a, ws: [b]]. Never saved — only use it where
    /// the action fails before writing, or doesn't write at all.
    fn tree(cursor: &[usize]) -> Tree {
        let task = |name: &str| Node::Task(Task::new(name.into(), None, Utc::now()));
        let mut ws = Container::new("ws".into(), "/tmp/ws".into(), ContainerKind::Workspace);
        ws.children = vec![task("b")];
        let mut root = Container::new(
            "root".into(),
            PathBuf::from("/tmp/root"),
            ContainerKind::Root,
        );
        root.children = vec![task("a"), Node::Container(ws)];
        Tree {
            root: Node::Container(root),
            cursor: cursor.to_vec(),
        }
    }

    // ---------- key mapping ----------

    #[test]
    fn no_key_is_bound_twice() {
        let mut seen = std::collections::HashSet::new();
        for b in KEYMAP {
            for k in b.keys {
                assert!(seen.insert(*k), "{k:?} is bound twice");
            }
        }
    }

    #[test]
    fn every_binding_has_help_text() {
        for b in KEYMAP {
            assert!(!b.help.is_empty(), "{:?} has no help text", b.action);
        }
    }

    #[test]
    fn help_key() {
        assert_eq!(action_for(press(KeyCode::Char('?'))), Some(Action::Help));
    }

    #[test]
    fn key_labels() {
        assert_eq!(key_label(&KeyCode::Char('j')), "j");
        assert_eq!(key_label(&KeyCode::Char(' ')), "space");
        assert_eq!(key_label(&KeyCode::Down), "↓");
    }

    #[test]
    fn status_keys() {
        let cases = [
            ('x', Finished),
            ('p', InProgress),
            ('s', Stale),
            ('u', Pending),
        ];
        for (key, status) in cases {
            assert_eq!(
                action_for(press(KeyCode::Char(key))),
                Some(Action::SetStatus(status)),
                "key {key:?}"
            );
        }
    }

    // ---------- apply ----------

    #[tokio::test]
    async fn navigation_moves_cursor_without_message() {
        let mut t = tree(&[0]);
        let msg = Action::Down.apply(&mut t).await.unwrap();
        assert_eq!(msg, None);
        assert_eq!(t.cursor, vec![1]);
    }

    #[tokio::test]
    async fn set_status_on_container_is_error() {
        let mut t = tree(&[1]); // "ws"
        let err = Action::SetStatus(Finished).apply(&mut t).await.unwrap_err();
        assert!(err.to_string().contains("only tasks"), "got: {err}");
    }

    #[tokio::test]
    async fn set_status_without_selection_is_error() {
        let mut t = tree(&[]); // cursor on the root (e.g. empty tree)
        assert!(Action::SetStatus(Finished).apply(&mut t).await.is_err());
    }

    #[tokio::test]
    async fn set_status_updates_task_and_reports_it() {
        let tmp = tempfile::tempdir().unwrap();
        let mut t = Tree::load_from(tmp.path()).await.unwrap(); // real, empty root
        let path = t
            .create_task(&[], Task::new("sheet".into(), None, Utc::now()))
            .await
            .unwrap();
        t.cursor = path.clone();

        let msg = Action::SetStatus(Finished).apply(&mut t).await.unwrap();

        assert_eq!(msg.as_deref(), Some("sheet -> Finished"));
        let Some(Node::Task(task)) = t.get(&path) else {
            panic!("expected a task at {path:?}");
        };
        assert_eq!(task.status, Finished);
    }

    #[test]
    fn vim_and_arrow_keys_map_the_same() {
        assert_eq!(action_for(press(KeyCode::Char('j'))), Some(Action::Down));
        assert_eq!(action_for(press(KeyCode::Down)), Some(Action::Down));
        assert_eq!(action_for(press(KeyCode::Char('h'))), Some(Action::Out));
        assert_eq!(action_for(press(KeyCode::Left)), Some(Action::Out));
    }

    #[test]
    fn quit_keys() {
        assert_eq!(action_for(press(KeyCode::Char('q'))), Some(Action::Quit));
        assert_eq!(action_for(press(KeyCode::Esc)), Some(Action::Quit));
    }

    #[test]
    fn unknown_key_is_none() {
        assert_eq!(action_for(press(KeyCode::Char('#'))), None);
        assert_eq!(action_for(press(KeyCode::F(5))), None);
    }

    #[test]
    fn release_is_ignored() {
        let key = KeyEvent {
            kind: KeyEventKind::Release,
            state: KeyEventState::NONE,
            ..press(KeyCode::Char('j'))
        };
        assert_eq!(action_for(key), None);
    }
}
