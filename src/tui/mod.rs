//! Interactive tree browser (`udo` without a subcommand).
//!
//! - `app`:   state + key handling (tested without a terminal)
//! - `keys`:  key -> action table
//! - `toast`: expiring messages
//! - `tree_state`: cursor + folding of the tree pane
//! - `view`:  drawing
//!
//! This file only does terminal I/O: draw, wait, hand keys to `App`.

pub mod app;
pub mod form;
pub mod keys;
pub mod tick;
pub mod toast;
pub mod tree_state;
pub mod view;

use std::{io, time::Instant};

use crossterm::event::{Event, EventStream};
use futures_util::StreamExt;
use ratatui::DefaultTerminal;

use crate::{
    Res,
    core::Core,
    model::time,
    tui::{
        app::{App, Flow},
        tick::next_tick,
        tree_state::TreeState,
    },
};

/// Run the TUI until the user quits, then save the view (folded containers,
/// selected node).
pub async fn run(core: &mut Core) -> Res<()> {
    let tree_state = TreeState::load(core.tree()).await;
    let mut app = App::new(core, tree_state);
    app.reload().await;
    let mut terminal = ratatui::init();
    let result = event_loop(&mut terminal, &mut app).await;
    ratatui::restore(); // always, even if the loop failed
    result?;
    app.tree_state.save(app.core.tree()).await
}

/// Draw, wait, react, repeat. Only terminal errors end the loop.
async fn event_loop(terminal: &mut DefaultTerminal, app: &mut App<'_>) -> Res<()> {
    let mut events = EventStream::new();
    loop {
        terminal.draw(|f| view::draw(f, app, time::now()))?;
        let tick = Instant::now() + next_tick(time::now(), app.running.as_ref());
        let deadline = app.toast_deadline().map_or(tick, |t| t.min(tick));
        match next_wake(&mut events, deadline).await {
            Wake::Closed => return Ok(()),
            Wake::Timeout => {
                app.expire_toast(Instant::now());
                app.reload().await;
            }
            Wake::Event(event) => {
                // non-key events (e.g. resize) just lead to a redraw
                if let Event::Key(key) = event?
                    && app.handle_key(key).await == Flow::Quit
                {
                    return Ok(());
                }
            }
        }
    }
}

/// Why the loop woke up.
enum Wake {
    Event(io::Result<Event>),
    /// `deadline` passed: tick or toast expiry.
    Timeout,
    /// Event stream ended (terminal gone).
    Closed,
}

/// Wait for the next terminal event, but not past `deadline`.
async fn next_wake(events: &mut EventStream, deadline: Instant) -> Wake {
    let left = deadline.saturating_duration_since(Instant::now());
    match tokio::time::timeout(left, events.next()).await {
        Ok(Some(event)) => Wake::Event(event),
        Ok(None) => Wake::Closed,
        Err(_elapsed) => Wake::Timeout,
    }
}
