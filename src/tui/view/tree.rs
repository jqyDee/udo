//! Left pane: the tree as an indented, scrollable list.

use chrono::Local;
use ratatui::{
    Frame,
    layout::Rect,
    style::{Style, Stylize},
    text::{Line, Span},
    widgets::{Block, List, ListItem, Paragraph},
};

use crate::{
    DATE_FMT,
    model::{
        node::NodeBody,
        task::{Task, TaskStatus},
        tree::{Row, Tree},
    },
    tui::tree_state::TreeState,
};

pub fn draw(frame: &mut Frame, area: Rect, tree: &Tree, state: &mut TreeState) {
    let rows = state.rows(tree);
    state
        .list
        .select(rows.iter().position(|r| r.path == state.cursor));

    let block = Block::bordered().title(" udo ");
    if rows.is_empty() {
        let hint = Paragraph::new("Nothing here yet. Add some with `udo create-workspace`.")
            .block(block)
            .dim();
        frame.render_widget(hint, area);
    } else {
        let lines = rows.iter().map(|r| row_line(r, state.is_collapsed(r.node)));
        let list_widget = List::new(lines.map(ListItem::new))
            .block(block)
            .highlight_style(Style::new().reversed());
        frame.render_stateful_widget(list_widget, area, &mut state.list);
    }
}

fn row_line<'a>(row: &Row<'a>, folded: bool) -> Line<'a> {
    let indent = Span::raw("  ".repeat(row.depth));
    let name = row.node.name();
    match &row.node.body {
        NodeBody::Container(c) => {
            let marker = match (c.children.is_empty(), folded) {
                (true, _) => "  ",
                (false, true) => "▸ ",
                (false, false) => "▾ ",
            };
            Line::from(vec![
                indent,
                Span::raw(marker),
                Span::raw(format!("{name}/")).bold().blue(),
            ])
        }
        NodeBody::Task(t) => Line::from(vec![
            indent,
            Span::raw(format!("{} ", status_icon(&t.status))),
            task_name(name, t),
            // stored as UTC, shown local (same as entered in the form)
            Span::raw(format!(
                "  {}",
                t.due_date.with_timezone(&Local).format(DATE_FMT)
            ))
            .dim(),
        ]),
    }
}

fn task_name<'a>(name: &'a str, t: &Task) -> Span<'a> {
    let name = Span::raw(name);
    match t.status {
        TaskStatus::Finished => name.dim().crossed_out(),
        TaskStatus::Stale => name.red(),
        _ => name,
    }
}

fn status_icon(s: &TaskStatus) -> &'static str {
    match s {
        TaskStatus::Pending => "○",
        TaskStatus::InProgress => "◐",
        TaskStatus::Finished => "●",
        TaskStatus::Stale => "!",
    }
}
