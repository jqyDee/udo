//! Interactive tree browser (`udo` without a subcommand). Prototype.

pub mod events;
pub mod layout;
pub mod state;

use std::time::Instant;

use crossterm::event::{Event, EventStream, KeyEventKind};
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

/// Draw, wait for the next event, handle it. Action results/errors become a
/// toast that expires on its own; only terminal errors end the loop.
async fn event_loop(terminal: &mut DefaultTerminal, tree: &mut Tree) -> Res<()> {
    let mut state = UiState::default(); // kept across frames: scroll offset, toast, help
    let mut events = EventStream::new();
    loop {
        terminal.draw(|f| layout::draw(f, tree, &mut state))?;

        // Toast showing: wait at most until it expires, then redraw without
        // it. No toast: wait for the next event, no timer at all.
        let next = match state.status_until {
            Some(until) => {
                let left = until.saturating_duration_since(Instant::now());
                match tokio::time::timeout(left, events.next()).await {
                    Ok(next) => next,
                    Err(_elapsed) => {
                        state.expire(Instant::now());
                        continue;
                    }
                }
            }
            None => events.next().await,
        };
        let Some(event) = next else {
            return Ok(());
        };
        let Event::Key(key) = event? else {
            continue;
        };
        if key.kind != KeyEventKind::Press {
            continue;
        }
        if state.show_help {
            state.show_help = false; // any key closes the help
            continue;
        }
        let Some(action) = action_for(key) else {
            continue;
        };

        match action {
            Action::Quit => return Ok(()),
            Action::Help => {
                state.show_help = true;
                continue;
            }
            _ => {}
        }
        match action.apply(tree).await {
            Ok(Some(msg)) => state.info(msg),
            Err(e) => state.error(e.to_string()),
            Ok(None) => {}
        }
    }
}
