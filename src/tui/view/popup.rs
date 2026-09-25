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
    keys::{Binding, KEYMAP, Section, bindings, key_label},
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
/// bindings indented below. Layout = first of these that fits the screen
/// height: one column with a blank line between sections, one column
/// without, two columns (sections split so both are about equally tall).
pub fn draw_help(frame: &mut Frame) {
    const TITLE: &str = " keys · any key closes ";
    const COLUMN_GAP: u16 = 3;

    // one key column for all sections, so the help texts line up everywhere
    let key_w = bindings()
        .map(|b| keys_text(b).chars().count())
        .max()
        .unwrap_or(0);
    let sections: Vec<Vec<Line>> = KEYMAP.iter().map(|s| section_lines(s, key_w)).collect();

    let area = frame.area();
    let fits = |rows: usize| rows + 2 <= area.height as usize; // + 2 border
    let columns: Vec<Vec<Line>> = if fits(stacked(&sections, true).len()) {
        vec![stacked(&sections, true)]
    } else if fits(stacked(&sections, false).len()) {
        vec![stacked(&sections, false)]
    } else {
        let (left, right) = sections.split_at(balanced_split(&sections));
        vec![stacked(left, true), stacked(right, true)]
    };

    // column widths = widest line (display width); box = columns + gaps
    // + 2 border + 2 padding, never narrower than the title
    let widths: Vec<u16> = columns
        .iter()
        .map(|c| c.iter().map(Line::width).max().unwrap_or(0) as u16)
        .collect();
    let gaps = COLUMN_GAP * (columns.len() as u16 - 1);
    let width = (widths.iter().sum::<u16>() + gaps + 4).max(TITLE.chars().count() as u16 + 2);
    let height = columns.iter().map(Vec::len).max().unwrap_or(0) as u16 + 2;

    let rect = centered(area, width, height);
    let block = Block::bordered()
        .title(TITLE)
        .padding(Padding::horizontal(1));
    let inner = block.inner(rect);
    frame.render_widget(Clear, rect); // wipe what's underneath
    frame.render_widget(block, rect);

    let cells = Layout::horizontal(widths.iter().map(|&w| Constraint::Length(w)))
        .spacing(COLUMN_GAP)
        .split(inner);
    for (lines, cell) in columns.into_iter().zip(cells.iter()) {
        frame.render_widget(Paragraph::new(lines), *cell);
    }
}

/// Heading + one line per binding, keys padded to `key_w`.
fn section_lines(section: &Section, key_w: usize) -> Vec<Line<'static>> {
    let mut lines = vec![Line::from(section.title).bold().yellow()];
    for b in section.bindings {
        lines.push(Line::from(vec![
            Span::raw(format!("  {:<key_w$}  ", keys_text(b))).bold(),
            Span::raw(b.help),
        ]));
    }
    lines
}

/// Sections one below the other, optionally with a blank line between.
fn stacked<'a>(sections: &[Vec<Line<'a>>], gaps: bool) -> Vec<Line<'a>> {
    let mut out = vec![];
    for (i, s) in sections.iter().enumerate() {
        if gaps && i > 0 {
            out.push(Line::default());
        }
        out.extend(s.iter().cloned());
    }
    out
}

/// Where to cut `sections` into two columns so the taller one is as short as
/// possible. The left column is never empty (unless there are no sections).
fn balanced_split(sections: &[Vec<Line>]) -> usize {
    (1..=sections.len())
        .min_by_key(|&k| {
            let (l, r) = sections.split_at(k);
            stacked(l, true).len().max(stacked(r, true).len())
        })
        .unwrap_or(0)
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
