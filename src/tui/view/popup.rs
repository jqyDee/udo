//! Overlays drawn on top of the panes: toast (top right), key help, confirm
//! prompt, full delete and picker (center), plus the geometry/text helpers
//! they share.

use ratatui::{
    Frame,
    layout::{Constraint, Flex, Layout, Rect},
    style::{Color, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, Clear, Padding, Paragraph},
};

use crate::{
    model::tree::PurgePlan,
    tui::{
        app::{Confirm, ConfirmAction, ConfirmStage},
        form::TextInput,
        keys::{Binding, Section, bindings_in, key_label},
        pick::{PickItem, Picker},
        toast::{Toast, ToastKind},
    },
};

use super::form::text_spans;

/// Width of the `folder:` / `also:` column in the full delete popup.
const LABEL_W: usize = 9;

/// Centered yes / no prompt, drawn by its stage from the texts in `c`.
pub fn draw_confirm(frame: &mut Frame, c: &Confirm) {
    match &c.stage {
        ConfirmStage::Ask => draw_ask(frame, c),
        ConfirmStage::TypeToConfirm { expected, input } => {
            draw_type_to_confirm(frame, c, expected, input)
        }
    }
}

/// Question, notes, keys. Only the listed keys do anything (see
/// `App::answer_confirm`), so the title must not promise "any key".
fn draw_ask(frame: &mut Frame, c: &Confirm) {
    let max_text_w = max_text_w(frame.area());
    let mut lines = question_lines(c, max_text_w);
    lines.extend(note_lines(c, max_text_w));
    lines.push(Line::default());
    lines.push(keys_line(c));
    draw_box(frame, c.title, lines, Color::Red);
}

/// Question, the action's detail rows, notes, then the text to type and
/// the keys.
fn draw_type_to_confirm(frame: &mut Frame, c: &Confirm, expected: &str, input: &TextInput) {
    let max_text_w = max_text_w(frame.area());
    let mut lines = question_lines(c, max_text_w);
    match &c.action {
        ConfirmAction::PurgeNode { plan } => lines.extend(purge_rows(plan, max_text_w)),
        ConfirmAction::RemoveNode { .. } | ConfirmAction::RemoveSession { .. } => {}
    }
    lines.extend(note_lines(c, max_text_w));
    lines.push(Line::from(c.type_prompt));
    // room for the whole text + cursor cell, if the screen allows
    let input_w = (expected.chars().count() + 1).min(max_text_w - 2);
    let mut input_line = vec![Span::raw("> ")];
    input_line.extend(text_spans(input, true, input_w));
    lines.push(Line::from(input_line));
    lines.push(Line::default());
    lines.push(keys_line(c));
    draw_box(frame, c.title, lines, Color::Red);
}

/// Centered picker: one row per item, the cursor's row reversed (and
/// marked `>`: readable without colors), notes dim, the keys below. More
/// items than fit: a window that keeps the cursor in view. It is not
/// stored: computed from the cursor, it only moves once the cursor would
/// leave it at the bottom.
pub fn draw_picker(frame: &mut Frame, p: &Picker) {
    let width = max_text_w(frame.area());
    // 1 margin + 2 border + blank + keys line + 1 margin
    let rows = (frame.area().height as usize).saturating_sub(6).max(1);
    let first = p.cursor.saturating_sub(rows - 1);
    let mut lines: Vec<Line> = p
        .items
        .iter()
        .enumerate()
        .skip(first)
        .take(rows)
        .map(|(i, item)| item_line(item, i == p.cursor, width))
        .collect();
    lines.push(Line::default());
    lines.push(Line::from("enter pick · esc back · ? keys").dim());
    draw_box(frame, &p.title, lines, Color::Blue);
}

/// `> label (default)`: marker, label, dim note; the cursor's row
/// reversed. A label too long for `width` is cut with `…` (task paths can
/// be long; the note stays).
fn item_line(item: &PickItem, selected: bool, width: usize) -> Line<'static> {
    let marker = if selected { "> " } else { "  " };
    let note = item.note.map(|n| format!(" {n}")).unwrap_or_default();
    let label_w = width.saturating_sub(marker.len() + note.chars().count());
    let mut spans = vec![Span::raw(marker), Span::raw(cut(&item.label, label_w))];
    if !note.is_empty() {
        spans.push(Span::raw(note).dim());
    }
    let line = Line::from(spans);
    if selected { line.reversed() } else { line }
}

/// `text` in at most `width` chars: cut with `…` at the end if longer.
fn cut(text: &str, width: usize) -> String {
    if text.chars().count() <= width {
        return text.to_string();
    }
    let mut short: String = text.chars().take(width.saturating_sub(1)).collect();
    short.push('…');
    short
}

/// Widest text line in a centered box: 1 cell of screen margin each side,
/// - 2 margin - 2 border - 2 padding.
fn max_text_w(area: Rect) -> usize {
    (area.width as usize).saturating_sub(6).max(10)
}

/// The question, bold; it may contain a long name: wrapped, never cut.
fn question_lines(c: &Confirm, width: usize) -> Vec<Line<'static>> {
    wrap_text(&c.question, width)
        .into_iter()
        .map(|l| Line::from(l).bold())
        .collect()
}

fn note_lines(c: &Confirm, width: usize) -> Vec<Line<'static>> {
    c.notes
        .iter()
        .flat_map(|n| wrap_text(n, width))
        .map(|l| Line::from(l).dim())
        .collect()
}

/// `y remove from udo · n/esc cancel`
fn keys_line(c: &Confirm) -> Line<'static> {
    let keys: Vec<String> = c
        .keys
        .iter()
        .map(|(k, help)| format!("{k} {help}"))
        .collect();
    Line::from(keys.join(" · "))
}

/// Full delete details: what goes to the Trash. Paths are cut by chars,
/// never at spaces: they must look exactly as typed.
fn purge_rows(plan: &PurgePlan, max_text_w: usize) -> Vec<Line<'static>> {
    const MAX_OUTSIDE: usize = 5;
    let path_w = max_text_w.saturating_sub(LABEL_W).max(10);

    let mut lines = labeled("folder:", &plan.dir.display().to_string(), path_w);
    for (i, other) in plan.outside.iter().take(MAX_OUTSIDE).enumerate() {
        let label = if i == 0 { "also:" } else { "" };
        lines.extend(labeled(label, &other.display().to_string(), path_w));
    }
    if plan.outside.len() > MAX_OUTSIDE {
        let more = format!("{:LABEL_W$}and {} more", "", plan.outside.len() - MAX_OUTSIDE);
        lines.push(Line::from(more).dim());
    }
    lines.push(Line::from(contains_text(plan.containers, plan.tasks)));
    lines
}

/// Box around `lines` with a `border` colored frame (red: a warning),
/// centered, as small as they and the title allow.
fn draw_box(frame: &mut Frame, title: &str, lines: Vec<Line>, border: Color) {
    // display width (not bytes: `·`, umlauts), + 2 border + 2 padding; the
    // title sits on the border, 2 cells less than the box
    let title_w = Line::from(title).width().saturating_sub(2);
    let text_w = lines
        .iter()
        .map(Line::width)
        .max()
        .unwrap_or(0)
        .max(title_w);
    let rect = centered(frame.area(), text_w as u16 + 4, lines.len() as u16 + 2);
    frame.render_widget(Clear, rect); // wipe what's underneath
    frame.render_widget(
        Paragraph::new(lines).block(
            Block::bordered()
                .title(title)
                .border_style(Style::new().fg(border))
                .padding(Padding::horizontal(1)),
        ),
        rect,
    );
}

/// `label` padded to `LABEL_W`, then `path` in `width`-char pieces;
/// following pieces are indented under the first.
fn labeled(label: &str, path: &str, width: usize) -> Vec<Line<'static>> {
    chunks(path, width)
        .into_iter()
        .enumerate()
        .map(|(i, part)| {
            let label = if i == 0 { label } else { "" };
            Line::from(vec![
                Span::raw(format!("{label:<LABEL_W$}")).dim(),
                Span::raw(part),
            ])
        })
        .collect()
}

/// `s` cut into pieces of at most `width` chars. Unlike `wrap_text` every
/// space is kept: for paths that have to be typed exactly.
fn chunks(s: &str, width: usize) -> Vec<String> {
    let chars: Vec<char> = s.chars().collect();
    chars
        .chunks(width.max(1))
        .map(|c| c.iter().collect())
        .collect()
}

/// `contains 2 containers, 1 task`; nothing below -> `contains no other nodes`.
fn contains_text(containers: usize, tasks: usize) -> String {
    let count = |n: usize, word: &str| format!("{n} {word}{}", if n == 1 { "" } else { "s" });
    if containers == 0 && tasks == 0 {
        return "contains no other nodes".into();
    }
    format!("contains {}, {}", count(containers, "container"), count(tasks, "task"))
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

/// Centered key list built from `keymap` (the open mode's, see
/// `Mode::keymap`): one heading per section, its
/// bindings indented below. Layout = first of these that fits the screen
/// height: one column with a blank line between sections, one column
/// without, two columns (sections split so both are about equally tall).
pub fn draw_help(frame: &mut Frame, keymap: &[Section]) {
    const TITLE: &str = " keys · any key closes ";
    const COLUMN_GAP: u16 = 3;

    // one key column for all sections, so the help texts line up everywhere
    let key_w = bindings_in(keymap)
        .map(|b| keys_text(b).chars().count())
        .max()
        .unwrap_or(0);
    let sections: Vec<Vec<Line>> = keymap.iter().map(|s| section_lines(s, key_w)).collect();

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

    #[test]
    fn chunks_keep_every_space() {
        assert_eq!(chunks("a  b c", 3), vec!["a  ", "b c"]);
        assert_eq!(chunks("äöü", 2), vec!["äö", "ü"]);
    }

    #[test]
    fn contains_text_counts_and_plurals() {
        assert_eq!(contains_text(2, 1), "contains 2 containers, 1 task");
        assert_eq!(contains_text(0, 0), "contains no other nodes");
    }
}
