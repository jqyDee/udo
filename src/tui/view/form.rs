//! Form view: renders active input fields and cursor for node creation/editing.

use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, Paragraph},
};

use crate::tui::form::{DateInput, FieldInput, Form, TextInput};

/// Width of the `" ▸ "` / `"   "` column in front of each field.
const PREFIX_W: usize = 3;

pub fn draw(frame: &mut Frame, area: Rect, form: &Form) {
    let mut lines = Vec::new();

    // label column = longest label + 1 space; the value gets the rest of the
    // inner width (- 2 border)
    let label_w = form
        .fields
        .iter()
        .map(|f| f.id.label().chars().count())
        .max()
        .unwrap_or(0);
    let value_w = (area.width as usize).saturating_sub(2 + PREFIX_W + label_w + 1);

    for (idx, field) in form.fields.iter().enumerate() {
        let is_active = idx == form.active_field;

        // Prefix arrow
        let prefix = if is_active {
            Span::styled(" ▸ ", Style::new().fg(Color::Yellow).bold())
        } else {
            Span::raw("   ")
        };

        let label = format!("{:<label_w$} ", field.id.label());
        let label = if is_active {
            Span::styled(label, Style::new().bold())
        } else {
            Span::styled(label, Style::new().dim())
        };

        let mut spans = vec![prefix, label];
        match &field.input {
            FieldInput::Text(t) => spans.extend(text_spans(t, is_active, value_w)),
            FieldInput::Date(d) => spans.extend(date_spans(d, is_active)),
        }
        lines.push(Line::from(spans));
    }

    // Bottom help instructions
    lines.push(Line::default());
    // date keys aren't obvious: explain them while a date field is active
    let date_active = matches!(
        form.fields.get(form.active_field).map(|f| &f.input),
        Some(FieldInput::Date(_))
    );
    if date_active {
        lines.push(Line::from(" ←→ part · ↑↓ change · t today ").dim());
    }
    lines.push(Line::from(" tab switch · enter confirm · esc cancel ").dim());

    let title = format!(" {} ", form.title);
    let block = Block::bordered()
        .title(title)
        .border_style(Style::new().fg(Color::Yellow));

    frame.render_widget(Paragraph::new(lines).block(block), area);
}

/// Text value in at most `width` cells; if active, with an inverted cursor
/// cell. Too long: the active value scrolls so the cursor stays visible,
/// inactive values and placeholders show their end (`…` + tail; for paths
/// the end is the interesting part). Counts chars as cells: fine for
/// umlauts, off for double-width chars (CJK, emoji).
fn text_spans(t: &TextInput, is_active: bool, width: usize) -> Vec<Span<'static>> {
    // >= 2: once scrolled, `…` and the cursor cell each need one
    let width = width.max(2);
    let placeholder = t.placeholder.as_deref().unwrap_or("(empty)");

    if !is_active {
        return if t.value.is_empty() {
            vec![Span::styled(tail(placeholder, width), Style::new().dim())]
        } else {
            vec![Span::raw(tail(&t.value, width))]
        };
    }

    let chars: Vec<char> = t.value.chars().collect();
    // clamp: `cursor` is a pub field, never trust it to index with
    let cursor = t.cursor.min(chars.len());
    // visible chars [start, end): the cursor cell (at `cursor`, possibly one
    // past the text) must be inside, at the right edge once scrolled
    let start = (cursor + 1).saturating_sub(width);
    let end = (start + width).min(chars.len());
    let text = |from: usize, to: usize| chars[from..to].iter().collect::<String>();

    let mut spans = vec![];
    if start > 0 {
        // first visible char becomes `…`: there is more on the left
        spans.push(Span::styled("…", Style::new().dim()));
        spans.push(Span::raw(text(start + 1, cursor)));
    } else {
        spans.push(Span::raw(text(start, cursor)));
    }

    if cursor < chars.len() {
        spans.push(Span::styled(
            chars[cursor].to_string(),
            Style::new().reversed(),
        ));
        spans.push(Span::raw(text(cursor + 1, end)));
    } else {
        // cursor block at the end
        spans.push(Span::styled(" ", Style::new().reversed()));
        if chars.is_empty() && t.placeholder.is_some() {
            let hint = tail(placeholder, width - 1);
            spans.push(Span::styled(hint, Style::new().dim()));
        }
    }
    spans
}

/// `s` if it fits in `width` chars, else `…` + its last `width - 1` chars.
fn tail(s: &str, width: usize) -> String {
    let n = s.chars().count();
    if n <= width {
        return s.to_string();
    }
    let keep = width.saturating_sub(1);
    let mut out = String::from("…");
    out.extend(s.chars().skip(n - keep));
    out
}

/// `2026-10-15 14:30  Thu`; if active, the current segment inverted.
/// Owned strings (`'static`): `s` is local, slices of it can't be returned.
fn date_spans(d: &DateInput, is_active: bool) -> Vec<Span<'static>> {
    let s = d.display();
    // weekday helps picking a day while stepping through dates
    let weekday = Span::raw(d.value.format("  %a").to_string()).dim();

    if !is_active {
        return vec![Span::raw(s), weekday];
    }

    // byte slicing is fine, DATE_FMT is ASCII
    let r = d.segment.range();
    vec![
        Span::raw(s[..r.start].to_string()),
        Span::styled(s[r.start..r.end].to_string(), Style::new().reversed()),
        Span::raw(s[r.end..].to_string()),
        weekday,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What `text_spans` puts on screen, as one string.
    fn shown(t: &TextInput, is_active: bool, width: usize) -> String {
        text_spans(t, is_active, width)
            .iter()
            .map(|s| s.content.as_ref())
            .collect()
    }

    fn at(value: &str, cursor: usize) -> TextInput {
        let mut t = TextInput::new(value);
        t.cursor = cursor;
        t
    }

    #[test]
    fn tail_keeps_short_and_cuts_long_from_the_left() {
        assert_eq!(tail("abc", 5), "abc");
        assert_eq!(tail("abcdef", 4), "…def");
        assert_eq!(tail("äöüß", 3), "…üß"); // chars, not bytes
    }

    #[test]
    fn short_active_value_is_not_scrolled() {
        assert_eq!(shown(&at("abc", 3), true, 10), "abc "); // + cursor block
        assert_eq!(shown(&at("abc", 1), true, 10), "abc");
    }

    #[test]
    fn long_active_value_scrolls_to_keep_cursor_visible() {
        let value = "0123456789";
        // cursor at the end: last 4 cells = `…`, 8, 9, cursor block
        assert_eq!(shown(&at(value, 10), true, 4), "…89 ");
        // cursor on '6': window ends at the cursor cell
        assert_eq!(shown(&at(value, 6), true, 4), "…456");
        // cursor near the start: no scrolling, cut on the right
        assert_eq!(shown(&at(value, 1), true, 4), "0123");
    }

    #[test]
    fn scrolled_output_never_exceeds_width() {
        let value = "a fairly long value with äöü in it";
        for cursor in 0..=value.chars().count() {
            let n = shown(&at(value, cursor), true, 8).chars().count();
            assert!(n <= 8, "cursor {cursor}: {n} cells");
        }
    }

    #[test]
    fn inactive_long_value_and_placeholder_show_the_end() {
        assert_eq!(shown(&at("/very/long/path/dir", 0), false, 8), "…ath/dir");
        let empty = TextInput::new("").with_placeholder("/home/me/uni/<name>");
        assert_eq!(shown(&empty, false, 8), "…/<name>");
        assert_eq!(shown(&empty, true, 8), " …<name>"); // cursor block + hint
    }
}
