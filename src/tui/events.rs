//! Keys -> actions -> tree calls. Pure logic, no terminal.

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind};

use crate::model::tree::Tree;

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
        _ => return None,
    })
}

impl Action {
    /// Run the action on the tree. `Quit` is handled by the event loop.
    pub fn apply(self, tree: &mut Tree) {
        match self {
            Action::Quit => {}
            Action::Up => tree.move_up(),
            Action::Down => tree.move_down(),
            Action::In => tree.move_in(),
            Action::Out => tree.move_out(),
            Action::Toggle => tree.toggle_collapse(),
            Action::CollapseAll => tree.collapse_all(),
            Action::ExpandAll => tree.expand_all(),
        }
    }
}

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyEventState, KeyModifiers};

    use super::*;

    fn press(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
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
