//! Interactive tree browser (`udo` without a subcommand). Prototype.

pub mod events;
pub mod layout;
pub mod state;

use crossterm::event::{Event, EventStream};
use futures_util::StreamExt;
use ratatui::DefaultTerminal;

use crate::{
    Res,
    model::tree::Tree,
    tui::{
        events::{Action, action_for},
        state::UiState,
    },
};

/// Run the TUI until the user quits, then save the view (collapsed state).
pub async fn run(tree: &mut Tree) -> Res<()> {
    if tree.cursor.is_empty() {
        tree.move_down(); // select the first row
    }
    let mut terminal = ratatui::init();
    let result = event_loop(&mut terminal, tree).await;
    ratatui::restore(); // always, even if the loop failed
    result?;
    tree.save_view().await
}

/// Draw, wait for the next event, handle it. Action errors go to the status
/// line; only terminal errors end the loop.
async fn event_loop(terminal: &mut DefaultTerminal, tree: &mut Tree) -> Res<()> {
    let mut state = UiState::default(); // kept across frames: scroll offset + status
    let mut events = EventStream::new();
    loop {
        terminal.draw(|f| layout::draw(f, tree, &mut state))?;

        let Some(event) = events.next().await else {
            return Ok(());
        };
        let Event::Key(key) = event? else {
            continue;
        };
        let Some(action) = action_for(key) else {
            continue;
        };

        state.status = None; // key clears the message
        if action == Action::Quit {
            return Ok(());
        }
        if let Err(e) = action.apply(tree).await {
            state.error(e.to_string());
        }
    }
}
