//! Right pane: fields of the selected node.

use ratatui::{
    Frame,
    layout::Rect,
    style::Stylize,
    text::{Line, Span},
    widgets::{Block, Paragraph},
};

use crate::model::{node::Node, tree::Tree};

pub fn draw(frame: &mut Frame, area: Rect, tree: &Tree) {
    let lines = match tree.get(&tree.cursor) {
        Some(node) if !tree.cursor.is_empty() => detail_lines(node),
        _ => vec![Line::from("nothing selected").dim()],
    };
    frame.render_widget(
        Paragraph::new(lines).block(Block::bordered().title(" details ")),
        area,
    );
}

fn detail_lines(node: &Node) -> Vec<Line<'_>> {
    match node {
        Node::Container(c) => {
            let tasks = c
                .children
                .iter()
                .filter(|n| matches!(n, Node::Task(_)))
                .count();
            let mut lines = vec![
                Line::from(c.name.as_str()).bold(),
                Line::default(),
                field("kind", format!("{:?}", c.kind)),
                field("dir", c.dir.display().to_string()),
                field("tasks", tasks.to_string()),
                field("children", (c.children.len() - tasks).to_string()),
            ];
            if let Some(a) = &c.settings.archive_dir {
                lines.push(field("archive", a.display().to_string()));
            }
            if !c.unloaded.is_empty() {
                lines.push(field("missing", c.unloaded.len().to_string()).red());
            }
            lines
        }
        Node::Task(t) => vec![
            Line::from(t.name.as_str()).bold(),
            Line::default(),
            field("status", format!("{:?}", t.status)),
            field("due", t.due_date.format("%Y-%m-%d %H:%M").to_string()),
            field(
                "dir",
                t.dir
                    .as_ref()
                    .map_or("-".into(), |d| d.display().to_string()),
            ),
        ],
    }
}

/// `key` dimmed in a fixed-width column, then the value.
fn field(key: &str, value: String) -> Line<'_> {
    Line::from(vec![
        Span::raw(format!("{key:<10}")).dim(),
        Span::raw(value),
    ])
}
