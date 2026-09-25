//! Left pane: the tree as an indented, scrollable list.

use ratatui::{
    Frame,
    layout::Rect,
    style::{Style, Stylize},
    text::{Line, Span},
    widgets::{Block, List, ListItem, ListState, Paragraph},
};

use crate::model::{
    nav::Row,
    node::Node,
    task::{Task, TaskStatus},
    tree::Tree,
};

pub fn draw(frame: &mut Frame, area: Rect, tree: &Tree, list: &mut ListState) {
    let rows = tree.rows();
    list.select(rows.iter().position(|r| r.path == tree.cursor));

    let block = Block::bordered().title(" udo ");
    if rows.is_empty() {
        let hint = Paragraph::new("Nothing here yet. Add some with `udo create-workspace`.")
            .block(block)
            .dim();
        frame.render_widget(hint, area);
    } else {
        let list_widget = List::new(rows.iter().map(row_line).map(ListItem::new))
            .block(block)
            .highlight_style(Style::new().reversed());
        frame.render_stateful_widget(list_widget, area, list);
    }
}

fn row_line<'a>(row: &Row<'a>) -> Line<'a> {
    let indent = Span::raw("  ".repeat(row.depth));
    match row.node {
        Node::Container(c) => {
            let marker = match (c.children.is_empty(), c.collapsed) {
                (true, _) => "  ",
                (false, true) => "▸ ",
                (false, false) => "▾ ",
            };
            Line::from(vec![
                indent,
                Span::raw(marker),
                Span::raw(format!("{}/", c.name)).bold().blue(),
            ])
        }
        Node::Task(t) => Line::from(vec![
            indent,
            Span::raw(format!("{} ", status_icon(&t.status))),
            task_name(t),
            Span::raw(format!("  {}", t.due_date.format("%Y-%m-%d %H:%M"))).dim(),
        ]),
    }
}

fn task_name(t: &Task) -> Span<'_> {
    let name = Span::raw(t.name.as_str());
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
