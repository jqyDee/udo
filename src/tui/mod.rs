//! Interactive tree browser (`udo` without a subcommand).
//!
//! - `app`:   state + key handling (tested without a terminal)
//! - `keys`:  key -> action table
//! - `session_list`: page of the sessions tab
//! - `tick`:  when the loop wakes up without a key (timer, changes elsewhere)
//! - `toast`: expiring messages
//! - `tree_state`: cursor + folding of the tree pane
//! - `view`:  drawing
//!
//! This file only does terminal I/O: draw, wait, hand keys to `App`, and
//! hand the whole terminal to a run config when `App` asks (`Flow::Run`).

pub mod app;
pub mod form;
pub mod keys;
pub mod session_list;
pub mod tick;
pub mod toast;
pub mod tree_state;
pub mod view;

use std::{io, process::ExitStatus, time::Instant};

use crossterm::{
    cursor::{Hide, Show},
    event::{Event, EventStream},
    execute,
    terminal::{
        Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode,
        enable_raw_mode,
    },
};
use futures_util::StreamExt;
use ratatui::{DefaultTerminal, Terminal, backend::CrosstermBackend};

use crate::{
    Res,
    core::Core,
    model::time,
    run::{self, RunError, exit_code},
    tui::{
        app::{App, Flow, RunRequest},
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
                if let Event::Key(key) = event? {
                    match app.handle_key(key).await {
                        Flow::Continue => {}
                        Flow::Quit => return Ok(()),
                        Flow::Run(request) => {
                            let result = hand_over(terminal, &mut events, &request).await?;
                            app.after_run(&request.name, result).await;
                        }
                    }
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

/// Give the terminal to `request`'s script, then take it back. Its output
/// stays readable: a failed script asks for Enter before the TUI draws
/// over it. `events` is made anew afterwards, so the old reader cannot
/// have kept keys meant for the script. `Err`: the terminal itself failed
/// (ends the TUI, like a failed draw); the script's own result is the
/// inner one.
async fn hand_over(
    terminal: &mut DefaultTerminal,
    events: &mut EventStream,
    request: &RunRequest,
) -> Res<Result<ExitStatus, RunError>> {
    disable_raw_mode()?;
    execute!(io::stdout(), LeaveAlternateScreen, Show)?;

    let result = run::launch(&request.script, &request.ctx).await;
    if let Ok(status) = &result
        && !status.success()
    {
        println!(
            "\n[udo] {} exited with {}, press Enter to return",
            request.name,
            exit_code(*status)
        );
        wait_for_enter().await;
    }

    enable_raw_mode()?;
    execute!(io::stdout(), EnterAlternateScreen, Clear(ClearType::All), Hide)?;
    // a new `Terminal`, not `terminal.clear()`: that asks the terminal
    // where its cursor is (a round trip through stdin, failing where
    // nothing answers). A new one only reads the size (the window may
    // have changed meanwhile) and starts empty, so the next draw is whole.
    *terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
    *events = EventStream::new();
    Ok(result)
}

/// Block until a line is read from stdin (cooked mode: Enter ends it).
async fn wait_for_enter() {
    let _ = tokio::task::spawn_blocking(|| std::io::stdin().read_line(&mut String::new())).await;
}
