//! Left pane: the tree as an indented, scrollable list.

use chrono::Local;
use ratatui::{
    Frame,
    layout::Rect,
    style::{Style, Stylize},
    text::{Line, Span},
    widgets::{Block, List, ListItem},
};

use crate::{
    DATE_FMT,
    model::{
        node::NodeBody,
        task::{Task, TaskStatus},
        tree::{Row, Tree},
    },
    tui::{tree_state::TreeState, view::TaskInfo},
};

pub fn draw(frame: &mut Frame, area: Rect, tree: &Tree, state: &mut TreeState, info: &TaskInfo) {
    let rows = state.rows(tree);
    state
        .list
        .select(rows.iter().position(|r| r.path == state.cursor));

    let mut lines: Vec<Line> = rows
        .iter()
        .map(|r| row_line(r, state.is_collapsed(r.node), info))
        .collect();
    // only the root row: say how to start (after the rows, so list indexes
    // still match `rows`)
    if tree.root.children().is_empty() {
        lines.push(Line::from("  Nothing here yet: c new container, t new task").dim());
    }
    let list_widget = List::new(lines.into_iter().map(ListItem::new))
        .block(Block::bordered().title(" udo "))
        .highlight_style(Style::new().reversed());
    frame.render_stateful_widget(list_widget, area, &mut state.list);
}

fn row_line<'a>(row: &Row<'a>, folded: bool, info: &TaskInfo) -> Line<'a> {
    let name = row.node.name();
    // the root: a header row, not a sibling of its children
    if row.path.is_empty() {
        let dir = row.node.dir().map(|d| d.display().to_string());
        return Line::from(vec![
            Span::raw(name).bold(),
            Span::raw(format!("  {}", dir.unwrap_or_default())).dim(),
        ]);
    }
    let indent = Span::raw("  ".repeat(row.depth));
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
                Span::raw(format!("{name}/")).bold(),
            ])
        }
        NodeBody::Task(t) => {
            let status = info.status(row.node, t);
            Line::from(vec![
                indent,
                Span::raw(format!("{} ", status_icon(status))),
                task_name(name, t, status, info),
                // shown in the current local time (same as entered in the form)
                Span::raw("  "),
                Span::raw(format!("{}", t.due_date.with_timezone(&Local).format(DATE_FMT))).dim(),
            ])
        }
    }
}

/// Done: dimmed + crossed out; overdue: red (a done task is never overdue).
fn task_name<'a>(name: &'a str, t: &Task, status: TaskStatus, info: &TaskInfo) -> Span<'a> {
    let name = Span::raw(name);
    if status == TaskStatus::Done {
        name.dim().crossed_out()
    } else if t.is_overdue(info.now) {
        name.red()
    } else {
        name
    }
}

fn status_icon(s: TaskStatus) -> &'static str {
    match s {
        TaskStatus::ToDo => "○",
        TaskStatus::Started => "◐",
        TaskStatus::Done => "●",
    }
}
