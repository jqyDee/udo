//! Key -> action mapping. Pure data + lookup, no tree, no terminal.
//!
//! `KEYMAP` is the single source of truth: `action_for` looks keys up in it
//! and the help overlay is generated from it (one heading per `Section`).
//! New key = one line in the right section, plus handling the new `Action`
//! in `App::run`.

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use crate::model::task::TaskStatus::{self, Finished, InProgress, Pending, Stale};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Quit,
    Help,
    Up,
    Down,
    In,
    Out,
    Toggle,
    CollapseAll,
    ExpandAll,
    SetStatus(TaskStatus),
    Delete,
    NewContainer { global: bool },
    NewTask { global: bool },
}

/// One row of the keymap: all keys that trigger `action`, and its help text.
pub struct Binding {
    pub keys: &'static [KeyCode],
    pub action: Action,
    pub help: &'static str,
}

/// A titled group of bindings: one heading in the help overlay.
pub struct Section {
    pub title: &'static str,
    pub bindings: &'static [Binding],
}

/// All key bindings, grouped. Order here = order in the help overlay.
#[rustfmt::skip] // keep one binding per line, like a table
pub const KEYMAP: &[Section] = &[
    Section { title: "move", bindings: &[
        Binding { keys: &[KeyCode::Char('j'), KeyCode::Down], action: Action::Down, help: "move down" },
        Binding { keys: &[KeyCode::Char('k'), KeyCode::Up], action: Action::Up, help: "move up" },
        Binding { keys: &[KeyCode::Char('l'), KeyCode::Right], action: Action::In, help: "open / go in" },
        Binding { keys: &[KeyCode::Char('h'), KeyCode::Left], action: Action::Out, help: "go to parent" },
    ]},
    Section { title: "fold", bindings: &[
        Binding { keys: &[KeyCode::Char(' '), KeyCode::Enter], action: Action::Toggle, help: "fold / unfold" },
        Binding { keys: &[KeyCode::Char('z')], action: Action::CollapseAll, help: "fold all" },
        Binding { keys: &[KeyCode::Char('Z')], action: Action::ExpandAll, help: "unfold all" },
    ]},
    Section { title: "task", bindings: &[
        Binding { keys: &[KeyCode::Char('x')], action: Action::SetStatus(Finished), help: "mark done" },
        Binding { keys: &[KeyCode::Char('p')], action: Action::SetStatus(InProgress), help: "mark in progress" },
        Binding { keys: &[KeyCode::Char('s')], action: Action::SetStatus(Stale), help: "mark stale" },
        Binding { keys: &[KeyCode::Char('u')], action: Action::SetStatus(Pending), help: "mark to do" },
        Binding { keys: &[KeyCode::Char('d')], action: Action::Delete, help: "remove from udo" },
    ]},
    Section {title: "create", bindings: &[
        Binding { keys: &[KeyCode::Char('c')], action: Action::NewContainer {global: false}, help: "new container here" },
        Binding { keys: &[KeyCode::Char('C')], action: Action::NewContainer {global: true}, help: "new container global" },
        Binding { keys: &[KeyCode::Char('t')], action: Action::NewTask {global: false}, help: "new task here" },
        Binding { keys: &[KeyCode::Char('T')], action: Action::NewTask {global: true}, help: "new task global" },
    ]},
    Section { title: "app", bindings: &[
        Binding { keys: &[KeyCode::Char('?')], action: Action::Help, help: "toggle this help" },
        Binding { keys: &[KeyCode::Char('q')], action: Action::Quit, help: "quit" },
    ]},
];

/// All bindings of all sections, in `KEYMAP` order.
pub fn bindings() -> impl Iterator<Item = &'static Binding> {
    KEYMAP.iter().flat_map(|s| s.bindings)
}

/// Map a key press to an action via `KEYMAP`. Releases/repeats and unknown
/// keys -> None.
pub fn action_for(key: KeyEvent) -> Option<Action> {
    if key.kind != KeyEventKind::Press {
        return None;
    }
    bindings()
        .find(|b| b.keys.contains(&key.code))
        .map(|b| b.action)
}

/// Should this key be typed into a form field? Ctrl+x / Alt+x are shortcuts,
/// not text. Ctrl+Alt together is AltGr on some terminals (e.g. `@` on a
/// German layout), so that still counts as text.
pub fn is_text_input(key: KeyEvent) -> bool {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let alt = key.modifiers.contains(KeyModifiers::ALT);
    ctrl == alt
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

#[cfg(test)]
mod tests {
    use crossterm::event::KeyEventState;

    use super::*;
    use crate::test_util::press;

    #[test]
    fn no_key_is_bound_twice() {
        // across sections too: `find` would silently take the first one
        let mut seen = std::collections::HashSet::new();
        for b in bindings() {
            for k in b.keys {
                assert!(seen.insert(*k), "{k:?} is bound twice");
            }
        }
    }

    #[test]
    fn every_section_has_title_and_bindings() {
        for s in KEYMAP {
            assert!(!s.title.is_empty(), "section without title");
            assert!(!s.bindings.is_empty(), "section {:?} is empty", s.title);
        }
    }

    #[test]
    fn bindings_flattens_all_sections_in_order() {
        let total: usize = KEYMAP.iter().map(|s| s.bindings.len()).sum();
        assert_eq!(bindings().count(), total);
        let first = &KEYMAP[0].bindings[0];
        assert_eq!(bindings().next().unwrap().action, first.action);
    }

    #[test]
    fn every_binding_has_help_text() {
        for b in bindings() {
            assert!(!b.help.is_empty(), "{:?} has no help text", b.action);
        }
    }

    #[test]
    fn vim_and_arrow_keys_map_the_same() {
        assert_eq!(action_for(press(KeyCode::Char('j'))), Some(Action::Down));
        assert_eq!(action_for(press(KeyCode::Down)), Some(Action::Down));
        assert_eq!(action_for(press(KeyCode::Char('h'))), Some(Action::Out));
        assert_eq!(action_for(press(KeyCode::Left)), Some(Action::Out));
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

    #[test]
    fn create_keys() {
        assert_eq!(
            action_for(press(KeyCode::Char('c'))),
            Some(Action::NewContainer { global: false })
        );
        assert_eq!(
            action_for(press(KeyCode::Char('C'))),
            Some(Action::NewContainer { global: true })
        );
        assert_eq!(
            action_for(press(KeyCode::Char('t'))),
            Some(Action::NewTask { global: false })
        );
        assert_eq!(
            action_for(press(KeyCode::Char('T'))),
            Some(Action::NewTask { global: true })
        );
    }

    #[test]
    fn help_and_quit_keys() {
        assert_eq!(action_for(press(KeyCode::Char('?'))), Some(Action::Help));
        assert_eq!(action_for(press(KeyCode::Char('q'))), Some(Action::Quit));
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

    #[test]
    fn key_labels() {
        assert_eq!(key_label(&KeyCode::Char('j')), "j");
        assert_eq!(key_label(&KeyCode::Char(' ')), "space");
        assert_eq!(key_label(&KeyCode::Down), "↓");
    }
}
