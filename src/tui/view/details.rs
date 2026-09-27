//! Right pane: fields of the selected node.

use chrono::Local;

use ratatui::{
    Frame,
    layout::Rect,
    style::Stylize,
    text::{Line, Span},
    widgets::{Block, Paragraph},
};

use crate::{
    DATE_FMT,
    model::{
        node::{Node, NodeBody},
        settings::view::EffectiveSetting,
        tree::Tree,
    },
    tui::{app::details::DetailsTab, view::LABEL_WIDTH},
};

/// Every tab starts with the same header (`header_lines`), then its own
/// lines. `node`: the node at the cursor, the root if the cursor is empty
/// (empty tree); None only for a cursor that points nowhere.
pub fn draw(
    frame: &mut Frame,
    area: Rect,
    tree: &Tree,
    node: Option<&Node>,
    tab: DetailsTab,
    settings: &[EffectiveSetting],
) {
    let mut lines = header_lines(node);
    lines.extend(match tab {
        DetailsTab::Info => node.map(detail_lines).unwrap_or_default(),
        DetailsTab::Settings => setting_lines(tree, settings),
    });
    let titles: Vec<Span> = DetailsTab::ALL
        .iter()
        .map(|t| {
            let s = Span::raw(format!(" {} ", t.title()));
            if *t == tab { s.reversed() } else { s.dim() }
        })
        .collect();
    frame.render_widget(
        Paragraph::new(lines).block(Block::bordered().title(Line::from(titles))),
        area,
    );
}

/// Top of every tab: the node's name, then a blank line.
fn header_lines(node: Option<&Node>) -> Vec<Line<'_>> {
    match node {
        Some(node) => vec![Line::from(node.name()).bold(), Line::default()],
        None => vec![Line::from("nothing selected").dim(), Line::default()],
    }
}

/// Info tab: the node's own fields (below the shared header).
fn detail_lines(node: &Node) -> Vec<Line<'_>> {
    let kind = match node.body {
        NodeBody::Container(_) => "container",
        NodeBody::Task(_) => "task",
    };
    let mut lines = vec![
        field("type", kind.into()),
        field("id", node.id().to_string()),
        field(
            "created at",
            node.header
                .created_at
                .with_timezone(&Local)
                .format(DATE_FMT)
                .to_string(),
        ),
        field(
            "description",
            node.header
                .description
                .as_deref()
                .unwrap_or("-")
                .to_string(),
        ),
        Line::default(),
    ];
    match &node.body {
        NodeBody::Container(c) => {
            let tasks = c.children.iter().filter(|n| n.as_task().is_some()).count();
            lines.extend([
                field("kind", c.kind.to_string()),
                field("dir", c.dir.display().to_string()),
                field("tasks", tasks.to_string()),
                field("children", (c.children.len() - tasks).to_string()),
            ]);
            if !c.unloaded.is_empty() {
                lines.push(field("missing", c.unloaded.len().to_string()).red());
            }
        }
        NodeBody::Task(t) => lines.extend([
            field("status", t.status.to_string()),
            // shown in the current local time (same as entered in the form)
            field(
                "due",
                t.due_date
                    .with_timezone(&Local)
                    .format(DATE_FMT)
                    .to_string(),
            ),
            field(
                "dir",
                t.dir
                    .as_ref()
                    .map_or("-".into(), |d| d.display().to_string()),
            ),
        ]),
    }
    lines
}

/// `key` dimmed in a fixed-width column, then the value.
fn field(key: &str, value: String) -> Line<'_> {
    Line::from(vec![
        Span::raw(format!("{key:<LABEL_WIDTH$}")).dim(),
        Span::raw(value),
    ])
}

/// One line per setting: label, effective value, where it came from.
fn setting_lines(tree: &Tree, settings: &[EffectiveSetting]) -> Vec<Line<'static>> {
    settings
        .iter()
        .map(|s| match &s.value {
            None => field(s.label, "-".into()),
            Some(r) => {
                let mut line = field(s.label, r.value.clone());
                line.push_span(Span::raw(format!(" ({})", tree.source_text(&r.source))).dim());
                line
            }
        })
        .collect()
}
