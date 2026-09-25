//! Keys -> actions -> tree calls. Pure logic, no terminal.

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
}

/// Map a key press to an action. Releases/repeats and unknown keys -> None.
pub fn action_for(key: KeyEvent) -> Option<Action> {
    if key.kind != KeyEventKind::Press {
        return None;
    }
    Some(match key.code {
        KeyCode::Char('q') | KeyCode::Esc => Action::Quit,
        KeyCode::Char('j') | KeyCode::Down => Action::Down,
        KeyCode::Char('k') | KeyCode::Up => Action::Up,
        KeyCode::Char('l') | KeyCode::Right => Action::In,
        KeyCode::Char('h') | KeyCode::Left => Action::Out,
        KeyCode::Char(' ') | KeyCode::Enter => Action::Toggle,
        KeyCode::Char('z') => Action::CollapseAll,
        KeyCode::Char('Z') => Action::ExpandAll,
        KeyCode::Char('x') => Action::SetStatus(Finished),
        KeyCode::Char('p') => Action::SetStatus(InProgress),
        KeyCode::Char('s') => Action::SetStatus(Stale),
        KeyCode::Char('u') => Action::SetStatus(Pending),
        _ => return None,
    })
}

impl Action {
    /// Run the action on the tree. `Quit` is handled by the event loop.
    pub async fn apply(self, tree: &mut Tree) -> Res<Option<String>> {
        match self {
            Action::Quit => {}
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
        let mut root = Container::new("root".into(), PathBuf::from("/tmp/root"), ContainerKind::Root);
        root.children = vec![task("a"), Node::Container(ws)];
        Tree {
            root: Node::Container(root),
            cursor: cursor.to_vec(),
        }
    }

    // ---------- key mapping ----------

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
        assert_eq!(action_for(press(KeyCode::Char('?'))), None);
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
