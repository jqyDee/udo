//! Overlays drawn on top of the panes: toast (top right), key help and
//! confirm prompt (center), plus the geometry/text helpers they share.

use ratatui::{
    Frame,
    layout::{Constraint, Flex, Layout, Rect},
    style::{Color, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, Clear, Padding, Paragraph},
};

use crate::tui::{
    app::Confirm,
    keys::{Binding, KEYMAP, bindings, key_label},
    toast::{Toast, ToastKind},
};

/// Centered "remove?" prompt for `c`. Only y / n / esc do anything (see
/// `App::answer_confirm`), so the title must not promise "any key".
pub fn draw_confirm(frame: &mut Frame, c: &Confirm) {
    let area = frame.area();
    // keep 1 cell of screen margin: - 2 margin - 2 border - 2 padding
    let max_text_w = (area.width as usize).saturating_sub(6).max(10);

    // the question contains the (possibly long) name: wrap instead of cutting
    let mut lines: Vec<Line> = wrap_text(&format!("Remove \"{}\" from udo?", c.name), max_text_w)
        .into_iter()
        .map(|l| Line::from(l).bold())
        .collect();
    lines.push(Line::from("Files and folders stay on disk.").dim());
    lines.push(Line::default());
    lines.push(Line::from("y yes · n/esc no"));

    // display width (not bytes: `·`, umlauts), + 2 border + 2 padding
    let text_w = lines.iter().map(Line::width).max().unwrap_or(0);
    let rect = centered(area, text_w as u16 + 4, lines.len() as u16 + 2);
    frame.render_widget(Clear, rect); // wipe what's underneath
    frame.render_widget(
        Paragraph::new(lines).block(
            Block::bordered()
                .title(" remove? ")
                .border_style(Style::new().fg(Color::Red))
                .padding(Padding::horizontal(1)),
        ),
        rect,
    );
}

/// Small bordered message box in the top right (green info / red error).
pub fn draw_toast(frame: &mut Frame, toast: &Toast) {
    let (title, color) = match toast.kind {
        ToastKind::Info => (" ✓ ", Color::Green),
        ToastKind::Error => (" error ", Color::Red),
    };
    let area = frame.area();
    let max_text_w = (area.width / 2).saturating_sub(4).max(10) as usize;
    let lines = wrap_text(&toast.msg, max_text_w);
    let text_w = lines.iter().map(|l| l.chars().count()).max().unwrap_or(0);

    // +2 border, +2 padding
    let rect = toast_area(area, text_w as u16 + 4, lines.len() as u16 + 2);
    frame.render_widget(Clear, rect);
    frame.render_widget(
        Paragraph::new(lines.into_iter().map(Line::from).collect::<Vec<_>>()).block(
            Block::bordered()
                .title(title)
                .border_style(Style::new().fg(color))
                .padding(Padding::horizontal(1)),
        ),
        rect,
    );
}

/// Centered key list built from `KEYMAP`: one heading per section, its
/// bindings indented below, a blank line between sections.
pub fn draw_help(frame: &mut Frame) {
    const TITLE: &str = " keys · any key closes ";

    // one key column for all sections, so the help texts line up everywhere
    let key_w = bindings()
        .map(|b| keys_text(b).chars().count())
        .max()
        .unwrap_or(0);

    let mut lines: Vec<Line> = vec![];
    for (i, section) in KEYMAP.iter().enumerate() {
        if i > 0 {
            lines.push(Line::default());
        }
        lines.push(Line::from(section.title).bold().yellow());
        for b in section.bindings {
            lines.push(Line::from(vec![
                Span::raw(format!("  {:<key_w$}  ", keys_text(b))).bold(),
                Span::raw(b.help),
            ]));
        }
    }

    // widest line (display width) + 2 border + 2 padding; never narrower
    // than the title
    let text_w = lines.iter().map(Line::width).max().unwrap_or(0);
    let width = (text_w + 4).max(TITLE.chars().count() + 2) as u16;
    let rect = centered(frame.area(), width, lines.len() as u16 + 2);
    frame.render_widget(Clear, rect); // wipe what's underneath
    frame.render_widget(
        Paragraph::new(lines).block(
            Block::bordered()
                .title(TITLE)
                .padding(Padding::horizontal(1)),
        ),
        rect,
    );
}

/// All keys of a binding for display, e.g. `j/↓`.
fn keys_text(b: &Binding) -> String {
    b.keys.iter().map(key_label).collect::<Vec<_>>().join("/")
}

/// `width` x `height` rectangle in the middle of `area` (clamped to it).
fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let [area] = Layout::horizontal([Constraint::Length(width)])
        .flex(Flex::Center)
        .areas(area);
    let [area] = Layout::vertical([Constraint::Length(height)])
        .flex(Flex::Center)
        .areas(area);
    area
}

/// `width` x `height` box in the top-right corner of `area`, one cell in
/// from the edges, clamped so it never leaves `area`.
fn toast_area(area: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(area.width);
    let height = height.min(area.height);
    Rect {
        x: area.right().saturating_sub(width + 1).max(area.x),
        y: (area.y + 1).min(area.bottom().saturating_sub(height)),
        width,
        height,
    }
}

/// Greedy word wrap to at most `width` chars per line; longer words are split.
fn wrap_text(text: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut lines = vec![];
    let mut line = String::new();
    for word in text.split_whitespace() {
        let mut word: Vec<char> = word.chars().collect();
        // word too long for any line: cut it into width-sized pieces
        while word.len() > width {
            if !line.is_empty() {
                lines.push(std::mem::take(&mut line));
            }
            lines.push(word.drain(..width).collect());
        }
        if word.is_empty() {
            continue;
        }
        let needed = if line.is_empty() {
            word.len()
        } else {
            line.chars().count() + 1 + word.len()
        };
        if needed > width {
            lines.push(std::mem::take(&mut line));
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.extend(word);
    }
    if !line.is_empty() || lines.is_empty() {
        lines.push(line);
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toast_area_stays_inside_tiny_terminal() {
        let area = Rect::new(0, 0, 10, 3);
        let t = toast_area(area, 40, 8);
        assert!(t.right() <= area.right() && t.bottom() <= area.bottom());
    }

    #[test]
    fn toast_area_is_top_right_with_margin() {
        let t = toast_area(Rect::new(0, 0, 80, 24), 20, 3);
        assert_eq!((t.x, t.y, t.width, t.height), (59, 1, 20, 3));
    }

    #[test]
    fn centered_is_in_the_middle() {
        let c = centered(Rect::new(0, 0, 80, 24), 20, 4);
        assert_eq!((c.x, c.y, c.width, c.height), (30, 10, 20, 4));
    }

    #[test]
    fn wrap_text_breaks_on_words() {
        assert_eq!(wrap_text("aa bb cc", 5), vec!["aa bb", "cc"]);
    }

    #[test]
    fn wrap_text_splits_overlong_words() {
        assert_eq!(wrap_text("abcdefgh", 3), vec!["abc", "def", "gh"]);
    }

    #[test]
    fn wrap_text_empty_is_one_empty_line() {
        assert_eq!(wrap_text("", 10), vec![""]);
    }
}
