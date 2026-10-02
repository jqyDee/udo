//! Key -> action mapping. Pure data + lookup, no tree, no terminal.
//!
//! `KEYMAP` is the single source of truth: `action_for` looks keys up in it
//! and the help overlay is generated from it (one heading per `Section`).
//! New key = one line in the right section, plus handling the new `Action`
//! in `App::run`.

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

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
    ToggleDone,
    Edit,
    Delete,
    ToggleTimer,
    NewContainer,
    NewTask,
    NextTab,
    PrevTab,
    Back,
    /// Session list only: split the selected session.
    Split,
    /// Session list only: cut a part out of the selected session.
    Cut,
    /// Session list only: add a session by hand.
    Add,
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
    Section { title: "view", bindings: &[
        Binding { keys: &[KeyCode::Tab], action: Action::NextTab, help: "next details tab" },
        Binding { keys: &[KeyCode::BackTab], action: Action::PrevTab, help: "previous details tab" },
    ]},
    Section { title: "fold", bindings: &[
        Binding { keys: &[KeyCode::Char(' '), KeyCode::Enter], action: Action::Toggle, help: "fold / unfold" },
        Binding { keys: &[KeyCode::Char('z')], action: Action::CollapseAll, help: "fold all" },
        Binding { keys: &[KeyCode::Char('Z')], action: Action::ExpandAll, help: "unfold all" },
    ]},
    Section { title: "task", bindings: &[
        Binding { keys: &[KeyCode::Char('x')], action: Action::ToggleDone, help: "done / reopen" },
        Binding { keys: &[KeyCode::Char('s')], action: Action::ToggleTimer, help: "start / stop timer" },
        Binding { keys: &[KeyCode::Char('e')], action: Action::Edit, help: "edit" },
        Binding { keys: &[KeyCode::Char('d')], action: Action::Delete, help: "remove from udo" },
    ]},
    Section {title: "create", bindings: &[
        Binding { keys: &[KeyCode::Char('c')], action: Action::NewContainer, help: "new container" },
        Binding { keys: &[KeyCode::Char('t')], action: Action::NewTask, help: "new task" },
    ]},
    Section { title: "app", bindings: &[
        Binding { keys: &[KeyCode::Char('?')], action: Action::Help, help: "toggle this help" },
        Binding { keys: &[KeyCode::Char('q')], action: Action::Quit, help: "quit" },
    ]},
];

/// Keys while the cursor is in the sessions list (`Mode::Sessions`).
#[rustfmt::skip]
pub const SESSION_LIST_KEYMAP: &[Section] = &[
    Section { title: "sessions", bindings: &[
        Binding { keys: &[KeyCode::Char('j'), KeyCode::Down], action: Action::Down, help: "next (older) session" },
        Binding { keys: &[KeyCode::Char('k'), KeyCode::Up], action: Action::Up, help: "previous (newer) session" },
        Binding { keys: &[KeyCode::Char('l'), KeyCode::Right], action: Action::In, help: "next page" },
        Binding { keys: &[KeyCode::Char('h'), KeyCode::Left], action: Action::Out, help: "previous page" },
        Binding { keys: &[KeyCode::Char('a')], action: Action::Add, help: "add session" },
        Binding { keys: &[KeyCode::Char('e')], action: Action::Edit, help: "edit session" },
        Binding { keys: &[KeyCode::Char('d')], action: Action::Delete, help: "remove session" },
        Binding { keys: &[KeyCode::Char('s')], action: Action::Split, help: "split session" },
        Binding { keys: &[KeyCode::Char('c')], action: Action::Cut, help: "cut a part out" },
        Binding { keys: &[KeyCode::Esc], action: Action::Back, help: "back to the tree" },
    ]},
    Section { title: "app", bindings: &[
        // Binding { keys: &[KeyCode::Char('?')], action: Action::Help, help: "toggle this help" },
        Binding { keys: &[KeyCode::Char('q')], action: Action::Quit, help: "quit" },
    ]},
];

/// All bindings of all sections, in `KEYMAP` order.
pub fn bindings() -> impl Iterator<Item = &'static Binding> {
    bindings_in(KEYMAP)
}

/// All bindings of all sections of `keymap` (`KEYMAP`, `SESSION_LIST_KEYMAP`), in
/// order.
pub fn bindings_in(keymap: &[Section]) -> impl Iterator<Item = &'static Binding> {
    keymap.iter().flat_map(|s| s.bindings)
}

/// Map a key press to an action via `KEYMAP`. Releases/repeats and unknown
/// keys -> None.
pub fn action_for(key: KeyEvent) -> Option<Action> {
    action_for_in(KEYMAP, key)
}

/// Map a key press to an action via `keymap` (one per mode). Releases /
/// repeats and unknown keys -> None.
pub fn action_for_in(keymap: &[Section], key: KeyEvent) -> Option<Action> {
    if key.kind != KeyEventKind::Press {
        return None;
    }
    bindings_in(keymap)
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
        KeyCode::Tab => "tab".into(),
        KeyCode::BackTab => "shift + tab".into(),
        other => format!("{other:?}"),
    }
}

#[cfg(test)]
mod tests {
    use crossterm::event::KeyEventState;

    use super::*;
    use crate::test_util::press;

    /// Every keymap of a mode. Duplicates across them are fine (`e` means
    /// something else in the list), within one they are not.
    const KEYMAPS: [&[Section]; 2] = [KEYMAP, SESSION_LIST_KEYMAP];

    #[test]
    fn no_key_is_bound_twice() {
        for keymap in KEYMAPS {
            // across sections too: `find` would silently take the first one
            let mut seen = std::collections::HashSet::new();
            for b in bindings_in(keymap) {
                for k in b.keys {
                    assert!(seen.insert(*k), "{k:?} is bound twice");
                }
            }
        }
    }

    #[test]
    fn esc_goes_back_in_the_list() {
        assert_eq!(action_for_in(SESSION_LIST_KEYMAP, press(KeyCode::Esc)), Some(Action::Back));
        assert_eq!(action_for(press(KeyCode::Esc)), None); // the tree: nothing
    }

    #[test]
    fn every_section_has_title_and_bindings() {
        for s in KEYMAPS.into_iter().flatten() {
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
        for b in KEYMAPS.into_iter().flat_map(bindings_in) {
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

    /// Only "done" is set by hand; the old status keys `p` and `u` are gone,
    /// `s` is the timer now.
    #[test]
    fn done_and_timer_keys() {
        assert_eq!(action_for(press(KeyCode::Char('x'))), Some(Action::ToggleDone));
        assert_eq!(action_for(press(KeyCode::Char('s'))), Some(Action::ToggleTimer));
        for gone in ['p', 'u'] {
            assert_eq!(action_for(press(KeyCode::Char(gone))), None, "key {gone:?}");
        }
    }

    #[test]
    fn create_keys() {
        assert_eq!(action_for(press(KeyCode::Char('c'))), Some(Action::NewContainer));
        assert_eq!(action_for(press(KeyCode::Char('t'))), Some(Action::NewTask));
    }

    #[test]
    fn a_adds_in_the_session_list_only() {
        let a = press(KeyCode::Char('a'));
        assert_eq!(action_for_in(SESSION_LIST_KEYMAP, a), Some(Action::Add));
        assert_eq!(action_for(a), None); // the tree: nothing
    }

    #[test]
    fn s_splits_and_c_cuts_in_the_session_list() {
        let s = press(KeyCode::Char('s'));
        let c = press(KeyCode::Char('c'));
        assert_eq!(action_for_in(SESSION_LIST_KEYMAP, s), Some(Action::Split));
        assert_eq!(action_for_in(SESSION_LIST_KEYMAP, c), Some(Action::Cut));
        // the tree keeps its own meaning
        assert_eq!(action_for(s), Some(Action::ToggleTimer));
        assert_eq!(action_for(c), Some(Action::NewContainer));
    }

    #[test]
    fn d_removes_in_the_session_list() {
        let d = press(KeyCode::Char('d'));
        assert_eq!(action_for_in(SESSION_LIST_KEYMAP, d), Some(Action::Delete));
    }

    #[test]
    fn edit_and_delete_keys() {
        assert_eq!(action_for(press(KeyCode::Char('e'))), Some(Action::Edit));
        assert_eq!(action_for(press(KeyCode::Char('d'))), Some(Action::Delete));
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
