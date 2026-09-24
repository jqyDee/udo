//! Drawing: tree list (left), details (right), help line (bottom).

use ratatui::{
    Frame,
    layout::{Constraint, Layout},
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

const HELP: &str = " j/k move · l/h in/out · space fold · z/Z fold/unfold all · q quit";

pub fn draw(frame: &mut Frame, tree: &Tree, list_state: &mut ListState) {
    let [main, help] =
        Layout::vertical([Constraint::Fill(1), Constraint::Length(1)]).areas(frame.area());
    let [left, right] =
        Layout::horizontal([Constraint::Percentage(60), Constraint::Percentage(40)]).areas(main);

    // tree
    let rows = tree.rows();
    list_state.select(rows.iter().position(|r| r.path == tree.cursor));
    let block = Block::bordered().title(" udo ");
    if rows.is_empty() {
        let hint = Paragraph::new("Nothing here yet. Add some with `udo create-workspace`.")
            .block(block)
            .dim();
        frame.render_widget(hint, left);
    } else {
        let list = List::new(rows.iter().map(row_line).map(ListItem::new))
            .block(block)
            .highlight_style(Style::new().reversed());
        frame.render_stateful_widget(list, left, list_state);
    }

    // details
    let details = match tree.get(&tree.cursor) {
        Some(node) if !tree.cursor.is_empty() => detail_lines(node),
        _ => vec![Line::from("nothing selected").dim()],
    };
    frame.render_widget(
        Paragraph::new(details).block(Block::bordered().title(" details ")),
        right,
    );

    frame.render_widget(Line::from(HELP).dim(), help);
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

fn detail_lines(node: &Node) -> Vec<Line<'_>> {
    fn field<'a>(key: &'a str, value: String) -> Line<'a> {
        Line::from(vec![Span::raw(format!("{key:<10}")).dim(), Span::raw(value)])
    }
    match node {
        Node::Container(c) => {
            let tasks = c.children.iter().filter(|n| matches!(n, Node::Task(_))).count();
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

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use chrono::Utc;
    use ratatui::{Terminal, backend::TestBackend};

    use super::*;
    use crate::model::container::{Container, ContainerKind};

    fn container(name: &str, children: Vec<Node>) -> Node {
        let mut c = Container::new(
            name.into(),
            PathBuf::from("/tmp").join(name),
            ContainerKind::Workspace,
        );
        c.children = children;
        Node::Container(c)
    }

    /// Render one frame into a fake 80x12 terminal and return it as text.
    fn render(tree: &Tree) -> String {
        let mut terminal = Terminal::new(TestBackend::new(80, 12)).unwrap();
        let mut state = ListState::default();
        terminal.draw(|f| draw(f, tree, &mut state)).unwrap();
        let buf = terminal.backend().buffer().clone();
        buf.content().iter().map(|c| c.symbol()).collect()
    }

    #[test]
    fn renders_rows_details_and_help() {
        let tree = Tree {
            root: container(
                "root",
                vec![container(
                    "uni",
                    vec![Node::Task(Task::new("exam".into(), None, Utc::now()))],
                )],
            ),
            cursor: vec![0, 0],
        };
        let screen = render(&tree);
        assert!(screen.contains("▾ uni/"));
        assert!(screen.contains("○ exam"));
        assert!(screen.contains("Pending")); // details pane of the selected task
        assert!(screen.contains("q quit"));
    }

    #[test]
    fn renders_hint_on_empty_tree() {
        let tree = Tree {
            root: container("root", vec![]),
            cursor: vec![],
        };
        assert!(render(&tree).contains("Nothing here yet"));
    }
}
