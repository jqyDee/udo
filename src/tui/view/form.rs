//! Form view: renders active input fields and cursor for node creation/editing.

use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, Paragraph},
};

use crate::tui::form::Form;

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

        // Render value with cursor if active
        let mut spans = vec![prefix, label];

        let placeholder = field.placeholder.as_deref().unwrap_or("(empty)");

        if is_active {
            let chars: Vec<char> = field.value.chars().collect();
            // clamp: `cursor` is a pub field, never trust it to index with
            let cursor = field.cursor.min(chars.len());
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
                if chars.is_empty() && field.placeholder.is_some() {
                    spans.push(Span::styled(placeholder, Style::new().dim()));
                }
            }
        } else if field.value.is_empty() {
            spans.push(Span::styled(placeholder, Style::new().dim()));
        } else {
            spans.push(Span::raw(&field.value));
        }

        lines.push(Line::from(spans));
    }

    // Bottom help instructions
    lines.push(Line::default());
    lines.push(Line::from(" tab switch · enter confirm · esc cancel ").dim());

    let title = format!(" {} ", form.title);
    let block = Block::bordered()
        .title(title)
        .border_style(Style::new().fg(Color::Yellow));

    frame.render_widget(Paragraph::new(lines).block(block), area);
}
