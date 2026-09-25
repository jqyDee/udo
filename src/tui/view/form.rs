//! Form view: renders active input fields and cursor for node creation/editing.

use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, Paragraph},
};

use crate::tui::form::{DateInput, FieldInput, Form, TextInput};

pub fn draw(frame: &mut Frame, area: Rect, form: &Form) {
    let mut lines = Vec::new();

    for (idx, field) in form.fields.iter().enumerate() {
        let is_active = idx == form.active_field;

        // Prefix arrow
        let prefix = if is_active {
            Span::styled(" ▸ ", Style::new().fg(Color::Yellow).bold())
        } else {
            Span::raw("   ")
        };

        // Field label (padded to 8 chars)
        let label = if is_active {
            Span::styled(format!("{:<8} ", field.label), Style::new().bold())
        } else {
            Span::styled(format!("{:<8} ", field.label), Style::new().dim())
        };

        let mut spans = vec![prefix, label];
        match &field.input {
            FieldInput::Text(t) => spans.extend(text_spans(t, is_active)),
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

/// Text value; if active, with an inverted cursor cell.
fn text_spans(t: &TextInput, is_active: bool) -> Vec<Span<'_>> {
    let mut spans = vec![];
    let placeholder = t.placeholder.as_deref().unwrap_or("(empty)");

    if is_active {
        let chars: Vec<char> = t.value.chars().collect();
        // clamp: `cursor` is a pub field, never trust it to index with
        let cursor = t.cursor.min(chars.len());
        let before: String = chars[..cursor].iter().collect();
        spans.push(Span::raw(before));

        if cursor < chars.len() {
            // Character at cursor inverted
            spans.push(Span::styled(
                chars[cursor].to_string(),
                Style::new().reversed(),
            ));
            let after: String = chars[cursor + 1..].iter().collect();
            spans.push(Span::raw(after));
        } else {
            // Cursor block at the end
            spans.push(Span::styled(" ", Style::new().reversed()));
            if chars.is_empty() && t.placeholder.is_some() {
                spans.push(Span::styled(placeholder, Style::new().dim()));
            }
        }
    } else if t.value.is_empty() {
        spans.push(Span::styled(placeholder, Style::new().dim()));
    } else {
        spans.push(Span::raw(&t.value));
    }
    spans
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
