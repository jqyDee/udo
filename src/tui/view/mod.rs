//! Drawing. `draw` splits the screen and calls the pieces:
//! - `tree`:    tree list (left)
//! - `details`: selected node (right)
//! - `form`:    create / edit form (right, below the details)
//! - `popup`:   overlays: toast (top right), key help, confirm + picker
//!   (center)

mod details;
mod form;
mod popup;
#[cfg(test)]
mod tests;
mod tree;

use ratatui::{
    Frame,
    layout::{Constraint, Layout},
    style::Stylize,
    text::{Line, Span},
};

use std::collections::HashSet;

use crate::{
    estimate::{self, Estimate},
    model::{
        id::NodeId,
        node::Node,
        sessions::{Session, SessionSource},
        task::{Task, TaskStatus},
        time::Time,
    },
    tui::{
        app::{App, Mode},
        session_list::SessionList,
    },
};

/// Fixed hint; the full key list is the `?` overlay, generated from `KEYMAP`.
const HINT: &str = " ? help · q quit";
const LABEL_WIDTH: usize = 15;

/// What the panes need besides the tree: which tasks have sessions
/// (status), which task is running (if any), the time to compare due dates
/// with (overdue), the cursor node's sessions and estimate (details time
/// rows) and the page of the sessions tab.
pub struct ViewInfo<'a> {
    pub with_sessions: &'a HashSet<NodeId>,
    pub running: Option<NodeId>,
    pub now: Time,
    /// Sessions of the node at the cursor (details: time rows).
    pub sessions: &'a [Session],
    /// What udo estimates at the cursor (a task: its container's, without
    /// itself); `estimate::of_node`.
    pub estimate: Option<Estimate>,
    /// Page of the sessions tab (`page_len` already for this frame).
    pub session_list: SessionList,
    /// The keys go to the sessions list, not the tree, also under a form or
    /// prompt opened from it (`App::in_list`): the tree's cursor is drawn
    /// dimmed.
    pub tree_inactive: bool,
}

impl ViewInfo<'_> {
    fn status(&self, node: &Node, t: &Task) -> TaskStatus {
        t.status(self.with_sessions.contains(&node.id()))
    }
}

/// One frame. `now` comes from the caller (tests pass a fixed time).
pub fn draw(frame: &mut Frame, app: &mut App, now: Time) {
    let [main, bottom] =
        Layout::vertical([Constraint::Fill(1), Constraint::Length(1)]).areas(frame.area());
    let [left, right] =
        Layout::horizontal([Constraint::Percentage(35), Constraint::Percentage(65)]).areas(main);
    // an open form takes the lower part of the right pane
    let (details_area, form_area) = match &app.mode {
        Mode::Form(_) => {
            let [top, bottom] =
                Layout::vertical([Constraint::Percentage(40), Constraint::Percentage(60)])
                    .areas(right);
            (top, Some(bottom))
        }
        _ => (right, None),
    };
    // before `info`, which copies the list state; stored so the keys page
    // by what is on screen, and the page follows the selection
    let ids = app.newest_ids();
    app.session_list
        .set_page_len(details::page_len(details_area), &ids);

    let tree = app.core.tree();
    let info = ViewInfo {
        with_sessions: &app.with_sessions,
        running: app.running.as_ref().map(|s| s.task.id),
        now,
        sessions: &app.sessions,
        estimate: tree
            .get(&app.tree_state.cursor)
            .and_then(|n| estimate::of_node(n, &app.history)),
        session_list: app.session_list,
        tree_inactive: app.in_list(),
    };
    tree::draw(frame, left, tree, &mut app.tree_state, &info);

    // node at the cursor path; `[]` (empty tree) is the root, so every tab
    // shows the root then
    let node = tree.get(&app.tree_state.cursor);
    let settings = tree.effective_settings(&app.tree_state.cursor);
    details::draw(
        frame,
        details_area,
        tree,
        node,
        app.details_tab,
        &settings,
        &info,
    );
    if let (Mode::Form(form), Some(area)) = (&app.mode, form_area) {
        form::draw(frame, area, form);
    }

    let hint = Line::from(HINT).dim();
    let [timer_area, hint_area] = Layout::horizontal([
        Constraint::Fill(1),                     // timer: whatever is left
        Constraint::Length(hint.width() as u16), // hint: exactly as wide as its text
    ])
    .areas(bottom);

    if let Some(s) = &app.running {
        frame.render_widget(timer_line(s, now), timer_area);
    }
    frame.render_widget(hint, hint_area);

    // overlays last, so they lie on top; the toast above the picker (an
    // error must stay readable), help above the toast
    if let Mode::Pick(p) = &app.mode {
        popup::draw_picker(frame, p);
    }
    if let Some(toast) = &app.toast {
        popup::draw_toast(frame, toast);
    }
    if let Mode::Help(under) = &app.mode
        && let Some(keys) = under.keymap()
    {
        popup::draw_help(frame, keys);
    }
    if let Mode::Confirm(c) = &app.mode {
        popup::draw_confirm(frame, c);
    }
}

/// ` ▶ lab 3 · 1h12`; a program's session also names the program, dim
/// (` · tmux`): `s` stops what that program started (a manual stop wins).
fn timer_line(s: &Session, now: Time) -> Line<'static> {
    let mut spans = vec![Span::raw(format!(
        " ▶ {} · {}",
        s.task.name,
        s.duration(now)
    ))];
    // the variant, not the text: a new source must be decided on here
    match &s.source {
        SessionSource::Manual => {}
        SessionSource::Program(name) => spans.push(Span::raw(format!(" · {name}")).dim()),
    }
    Line::from(spans).green()
}
