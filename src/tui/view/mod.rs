//! Drawing. `draw` splits the screen and calls the pieces:
//! - `tree`:    tree list (left)
//! - `details`: selected node (right)
//! - `popup`:   overlays: toast (top right), key help (center)

mod details;
mod popup;
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

pub fn draw(frame: &mut Frame, app: &mut App) {
    let [main, bottom] =
        Layout::vertical([Constraint::Fill(1), Constraint::Length(1)]).areas(frame.area());
    let [left, right] =
        Layout::horizontal([Constraint::Percentage(60), Constraint::Percentage(40)]).areas(main);

    tree::draw(frame, left, app.tree, &mut app.list);
    details::draw(frame, right, app.tree);
    frame.render_widget(Line::from(HINT).dim(), bottom);

    // overlays last, so they lie on top; help above the toast
    if let Some(toast) = &app.toast {
        popup::draw_toast(frame, toast);
    }
    if app.mode == Mode::Help {
        popup::draw_help(frame);
    }
}

#[cfg(test)]
mod tests {
    use ratatui::{Terminal, backend::TestBackend};

    use super::*;
    use crate::{
        model::tree::Tree,
        test_util::{container, task, tree_with},
        tui::{keys::KEYMAP, toast::Toast},
    };

    /// Render one frame of `app` into a fake 80x24 terminal (24 rows: the
    /// help overlay must fit). One String per screen row, built from cells,
    /// so column indexes are real screen columns.
    fn render_rows(app: &mut App) -> Vec<String> {
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        terminal.draw(|f| draw(f, app)).unwrap();
        let buf = terminal.backend().buffer();
        (0..buf.area.height)
            .map(|y| (0..buf.area.width).map(|x| buf[(x, y)].symbol()).collect())
            .collect()
    }

    /// Whole screen as one string, for simple "is it there" checks.
    fn render(app: &mut App) -> String {
        render_rows(app).concat()
    }

    /// (row, column) of the first occurrence of `needle` on screen.
    fn find(rows: &[String], needle: &str) -> Option<(usize, usize)> {
        rows.iter().enumerate().find_map(|(y, row)| {
            let byte = row.find(needle)?;
            Some((y, row[..byte].chars().count()))
        })
    }

    fn empty_tree() -> Tree {
        tree_with(vec![], &[])
    }

    #[test]
    fn renders_rows_details_and_hint() {
        let mut t = tree_with(vec![container("uni", vec![task("exam")])], &[0, 0]);
        let screen = render(&mut App::new(&mut t));
        assert!(screen.contains("▾ uni/"));
        assert!(screen.contains("○ exam"));
        assert!(screen.contains("Pending")); // details pane of the selected task
        assert!(screen.contains("? help"));
    }

    #[test]
    fn renders_hint_on_empty_tree() {
        let mut t = empty_tree();
        assert!(render(&mut App::new(&mut t)).contains("Nothing here yet"));
    }

    #[test]
    fn error_toast_top_right_and_hint_stays() {
        let mut t = empty_tree();
        let mut app = App::new(&mut t);
        app.toast = Some(Toast::error("boom"));

        let rows = render_rows(&mut app);

        let (row, col) = find(&rows, "boom").expect("toast missing");
        assert!(row <= 2, "toast not at the top (row {row})");
        assert!(col >= 40, "toast not on the right (col {col})");
        assert!(rows[..4].iter().any(|r| r.contains("error")));
        assert!(rows.last().unwrap().contains("q quit"));
    }

    #[test]
    fn info_toast_top_right() {
        let mut t = empty_tree();
        let mut app = App::new(&mut t);
        app.toast = Some(Toast::info("saved"));

        let rows = render_rows(&mut app);

        let (row, col) = find(&rows, "saved").expect("toast missing");
        assert!(row <= 2 && col >= 40);
    }

    #[test]
    fn long_toast_wraps_instead_of_cutting_off() {
        let words = [
            "alpha", "bravo", "charlie", "delta", "echo", "foxtrot", "golf", "hotel", "india",
            "juliett", "kilo", "lima",
        ];
        let mut t = empty_tree();
        let mut app = App::new(&mut t);
        app.toast = Some(Toast::error(words.join(" ")));

        let rows = render_rows(&mut app);

        for w in words {
            assert!(find(&rows, w).is_some(), "{w} cut off");
        }
    }

    #[test]
    fn help_overlay_lists_every_binding() {
        let mut t = empty_tree();
        let mut app = App::new(&mut t);
        app.mode = Mode::Help;

        let screen = render(&mut app);

        for b in KEYMAP {
            assert!(screen.contains(b.help), "help for {:?} missing", b.action);
        }
    }

    #[test]
    fn help_overlay_hidden_in_normal_mode() {
        let mut t = empty_tree();
        assert!(!render(&mut App::new(&mut t)).contains("toggle this help"));
    }
}
