//! Keys while a picker is open (`Mode::Pick`). The picker itself
//! (`tui::pick`) knows only its items and cursor; what a pick leads to is
//! its `PickAction`, handled in `picked`.

use crossterm::event::KeyEvent;

use crate::tui::{
    app::{App, Flow, Mode},
    keys::Action,
    pick::Picker,
};

impl App<'_> {
    /// Keys while a picker is open, through `PICKER_KEYMAP`: move, `?`
    /// help over it, Esc back to the tree (nothing runs), Enter takes the
    /// item under the cursor (`picked`). Other keys do nothing.
    pub(super) fn handle_pick_key(&mut self, key: KeyEvent) -> Flow {
        // before borrowing the picker: `action_for` reads `self.mode` too
        let action = self.action_for(key);
        let Mode::Pick(picker) = &mut self.mode else {
            unreachable!("only called while Mode::Pick");
        };
        match action {
            Some(Action::Help) => self.open_help(),
            Some(Action::Down) => picker.down(),
            Some(Action::Up) => picker.up(),
            Some(Action::Back) => self.mode = Mode::Normal,
            Some(Action::Pick) => {
                // moved out, not cloned: the mode is `Normal` from here on,
                // `picked` may open the next picker (script -> task)
                let Mode::Pick(picker) = std::mem::take(&mut self.mode) else {
                    unreachable!("matched above");
                };
                return self.picked(*picker);
            }
            _ => {}
        }
        Flow::Continue
    }

    /// Enter in a picker; it is closed already (`Mode::Normal`). Its
    /// `PickAction` says what comes next: the task picker or `Flow::Run`.
    fn picked(&mut self, picker: Picker) -> Flow {
        let _ = picker; // placeholder for now!
        Flow::Continue
    }
}
