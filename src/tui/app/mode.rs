//! `Mode`: what the keys currently do, which keymap that is, and whether
//! the sessions list (not the tree) has them.

use crate::tui::{
    app::Confirm,
    form::Form,
    keys::{KEYMAP, PICKER_KEYMAP, SESSION_LIST_KEYMAP, Section},
    pick::Picker,
};

/// What the keys currently do. One mode at a time, so e.g. "help open and a
/// confirm prompt open" can't happen. New prompts = new variants here.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub enum Mode {
    /// Keys go through `KEYMAP`.
    #[default]
    Normal,
    /// Key help over `under`, the mode `?` was pressed in: shows its
    /// keymap; any key goes back to it. Boxed: a `Mode` can't hold a
    /// `Mode` directly (infinite size).
    Help(Box<Mode>),
    /// Yes / no prompt open; keys by its `ConfirmStage`, its
    /// `ConfirmAction` says what yes does.
    Confirm(Box<Confirm>),
    /// Create or edit form open; keys go to `Form::handle_key`.
    Form(Box<Form>),
    /// Cursor in the sessions tab's list (`SessionList::selected`; None:
    /// the list is empty): keys go through `SESSION_LIST_KEYMAP`.
    Sessions,
    /// Picker open (`o` on a container, `O`): keys go through
    /// `PICKER_KEYMAP`, its `PickAction` says what the pick leads to.
    Pick(Box<Picker>),
}

impl Mode {
    /// The keymap that handles keys in this mode, and that `?` shows.
    /// None: the mode takes keys itself (forms, prompts) or has none
    /// (help).
    pub fn keymap(&self) -> Option<&'static [Section]> {
        match self {
            Mode::Normal => Some(KEYMAP),
            Mode::Sessions => Some(SESSION_LIST_KEYMAP),
            Mode::Pick(_) => Some(PICKER_KEYMAP),
            Mode::Help(_) | Mode::Form(_) | Mode::Confirm(_) => None,
        }
    }

    /// The list (not the tree) has the keys, or gets them back when the
    /// open form / prompt / help closes: the tree's cursor is drawn dimmed.
    pub fn in_list(&self) -> bool {
        match self {
            Mode::Sessions => true,
            Mode::Form(form) => super::forms::mode_after(&form.action) == Mode::Sessions,
            Mode::Confirm(c) => super::confirm::mode_after(&c.action) == Mode::Sessions,
            Mode::Help(under) => under.in_list(),
            Mode::Normal | Mode::Pick(_) => false,
        }
    }
}
