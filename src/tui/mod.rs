//! Interactive tree browser (`udo` without a subcommand).
//!
//! - `app`:   state + key handling (tested without a terminal)
//! - `keys`:  key -> action table
//! - `toast`: expiring messages
//! - `view`:  drawing
//!
//! This file only does terminal I/O: draw, wait, hand keys to `App`.

pub mod app;
pub mod keys;
pub mod toast;
pub mod view;
pub mod form;

use std::{io, time::Instant};

use crossterm::event::{Event, EventStream};
use futures_util::StreamExt;
use ratatui::DefaultTerminal;

use crate::{
    Res,
    model::tree::Tree,
    tui::app::{App, Flow},
};

/// Run the TUI until the user quits, then save the view (collapsed state).
pub async fn run(tree: &mut Tree) -> Res<()> {
    let mut app = App::new(tree);
    let mut terminal = ratatui::init();
    let result = event_loop(&mut terminal, &mut app).await;
    ratatui::restore(); // always, even if the loop failed
    result?;
    app.tree.save_view().await
}

/// Draw, wait, react, repeat. Only terminal errors end the loop.
async fn event_loop(terminal: &mut DefaultTerminal, app: &mut App<'_>) -> Res<()> {
    let mut events = EventStream::new();
    loop {
        terminal.draw(|f| view::draw(f, app))?;
        match next_wake(&mut events, app.toast_deadline()).await {
            Wake::Closed => return Ok(()),
            Wake::Timeout => app.expire_toast(Instant::now()),
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
    /// `deadline` passed (toast should disappear).
    Timeout,
    /// Event stream ended (terminal gone).
    Closed,
}

/// Wait for the next terminal event, but not past `deadline`. No deadline:
/// wait as long as it takes, no timer at all.
async fn next_wake(events: &mut EventStream, deadline: Option<Instant>) -> Wake {
    let next = match deadline {
        Some(deadline) => {
            let left = deadline.saturating_duration_since(Instant::now());
            match tokio::time::timeout(left, events.next()).await {
                Ok(next) => next,
                Err(_elapsed) => return Wake::Timeout,
            }
        }
        None => events.next().await,
    };
    match next {
        Some(event) => Wake::Event(event),
        None => Wake::Closed,
    }
}
