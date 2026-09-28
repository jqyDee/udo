//! Drawing. `draw` splits the screen and calls the pieces:
//! - `tree`:    tree list (left)
//! - `details`: selected node (right)
//! - `form`:    create / edit form (right, below the details)
//! - `popup`:   overlays: toast (top right), key help + confirm (center)

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
    text::Line,
};

use crate::tui::app::{App, Mode};

/// Fixed hint; the full key list is the `?` overlay, generated from `KEYMAP`.
const HINT: &str = " ? help · q quit";
const LABEL_WIDTH: usize = 15;

pub fn draw(frame: &mut Frame, app: &mut App) {
    let [main, bottom] =
        Layout::vertical([Constraint::Fill(1), Constraint::Length(1)]).areas(frame.area());
    let [left, right] =
        Layout::horizontal([Constraint::Percentage(35), Constraint::Percentage(65)]).areas(main);

    let tree = app.core.tree();
    tree::draw(frame, left, tree, &mut app.tree_state);

    // node at the cursor path; `[]` (empty tree) is the root, so both tabs
    // show the root then
    let node = tree.get(&app.tree_state.cursor);
    let settings = tree.effective_settings(&app.tree_state.cursor);
    // Split right pane when form is active
    if let Mode::Form(form) = &app.mode {
        let [top_details, bottom_form] =
            Layout::vertical([Constraint::Percentage(40), Constraint::Percentage(60)]).areas(right);
        details::draw(frame, top_details, tree, node, app.details_tab, &settings);
        form::draw(frame, bottom_form, form);
    } else {
        details::draw(frame, right, tree, node, app.details_tab, &settings);
    }

    frame.render_widget(Line::from(HINT).dim(), bottom);

    // overlays last, so they lie on top; help above the toast
    if let Some(toast) = &app.toast {
        popup::draw_toast(frame, toast);
    }
    if app.mode == Mode::Help {
        popup::draw_help(frame);
    }
    if let Mode::Confirm(c) = &app.mode {
        popup::draw_confirm(frame, c);
    }
}
