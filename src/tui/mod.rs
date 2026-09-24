//! Interactive tree browser (`udo` without a subcommand). Prototype.

pub mod events;
pub mod layout;

use crossterm::event::{self, Event};
use ratatui::{DefaultTerminal, widgets::ListState};

use crate::{
    Res,
    model::tree::Tree,
    tui::events::{Action, action_for},
};

/// Run the TUI until the user quits, then save the view (collapsed state).
pub async fn run(tree: &mut Tree) -> Res<()> {
    if tree.cursor.is_empty() {
        tree.move_down(); // select the first row
    }
    let mut terminal = ratatui::init();
    let result = event_loop(&mut terminal, tree);
    ratatui::restore(); // always, even if the loop failed
    result?;
    tree.save_view().await
}

/// Sync for now: nothing in here awaits yet. Becomes async once actions
/// write to disk (delete, create).
fn event_loop(terminal: &mut DefaultTerminal, tree: &mut Tree) -> Res<()> {
    let mut list_state = ListState::default(); // kept across frames: scroll offset
    loop {
        terminal.draw(|f| layout::draw(f, tree, &mut list_state))?;
        if let Event::Key(key) = event::read()? {
            match action_for(key) {
                Some(Action::Quit) => return Ok(()),
                Some(action) => action.apply(tree),
                None => {}
            }
        }
    }
}
