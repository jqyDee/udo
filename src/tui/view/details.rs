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
        sessions::{Session, SessionSource, TimeSummary},
        settings::view::EffectiveSetting,
        time::{Left, Time},
        tree::Tree,
    },
    tui::{
        app::details::DetailsTab,
        view::{LABEL_WIDTH, ViewInfo},
    },
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
    info: &ViewInfo,
) {
    let mut lines = header_lines(node);
    lines.extend(match tab {
        DetailsTab::Info => node
            .map(|n| detail_lines(tree, n, info))
            .unwrap_or_default(),
        DetailsTab::Settings => setting_lines(tree, settings),
        DetailsTab::Sessions => node.map(|n| session_lines(n, info)).unwrap_or_default(),
    });
    let mut titles: Vec<Span> = Vec::new();
    for (i, t) in DetailsTab::ALL.iter().enumerate() {
        if i > 0 {
            titles.push(Span::raw("│").dim());
        }
        let spacer = Span::raw(" ");
        let s = Span::raw(t.title());
        titles.push(spacer.clone());
        titles.push(if *t == tab { s.reversed() } else { s.dim() });
        titles.push(spacer);
    }
    frame.render_widget(
        Paragraph::new(lines).block(Block::bordered().title(Line::from(titles))),
        area,
    );
}

/// Lines of a details tab around its content: top + bottom border, and the
/// header (`header_lines`: name + blank line).
const CHROME: u16 = 2 + 2;

/// Widest task name column in the sessions tab; longer names end in `…`,
/// so the time columns stay visible in a narrow pane.
const NAME_MAX: usize = 16;

/// How many session rows the sessions tab has room for in `area` (one line
/// is kept for `page x/y`). At least 1, so paging never divides by 0.
pub fn page_len(area: Rect) -> usize {
    area.height.saturating_sub(CHROME + 1).max(1) as usize
}

/// Top of every tab: the node's name, then a blank line.
fn header_lines(node: Option<&Node>) -> Vec<Line<'_>> {
    match node {
        Some(node) => vec![Line::from(node.name()).bold(), Line::default()],
        None => vec![Line::from("nothing selected").dim(), Line::default()],
    }
}

/// Info tab: the node's own fields (below the shared header).
fn detail_lines<'a>(tree: &Tree, node: &'a Node, info: &ViewInfo) -> Vec<Line<'a>> {
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
            field("status", info.status(node, t).to_string()),
            // shown in the current local time (same as entered in the form)
            field(
                "due",
                format!(
                    "{}{}",
                    t.due_date.with_timezone(&Local).format(DATE_FMT),
                    if t.is_overdue(info.now) {
                        " (overdue)"
                    } else {
                        ""
                    }
                ),
            ),
            field(
                "dir",
                t.dir
                    .as_ref()
                    .map_or("-".into(), |d| d.display().to_string()),
            ),
        ]),
    }
    lines.push(Line::default());
    lines.extend(time_lines(tree, node, info));
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

/// Estimate, duration and what is left (tasks); only duration (containers).
fn time_lines(tree: &Tree, node: &Node, info: &ViewInfo) -> Vec<Line<'static>> {
    let time = TimeSummary::of(info.sessions, info.now);
    let duration = field("duration", time.to_string());
    let Some(task) = node.as_task() else {
        return vec![duration]; // container: no estimate, no left
    };
    let estimate = match &info.estimate {
        Some(e) => format!("{} ({})", e.value, tree.source_text(&e.source)),
        None => "-".into(),
    };
    let mut lines = vec![field("estimate", estimate), duration];
    if task.done_at.is_none() {
        lines.push(
            match info
                .estimate
                .as_ref()
                .map(|e| Left::of(e.value, time.duration))
            {
                Some(Left::Left(m)) => field("left", m.to_string()),
                Some(Left::Over(m)) => field("left", format!("over by {m}")).red(),
                None => field("left", "-".into()),
            },
        );
    }
    lines
}

/// Sessions tab: the cursor node's sessions, newest first, one page (the
/// page line only with several pages), the selected one reversed. A
/// container: with the task name in front.
fn session_lines(node: &Node, info: &ViewInfo) -> Vec<Line<'static>> {
    if info.sessions.is_empty() {
        return vec![Line::from("no session recorded yet").dim()];
    }
    let newest_first: Vec<&Session> = info.sessions.iter().rev().collect();
    let list = info.session_list;
    let page = &newest_first[list.range(newest_first.len())];

    // container / root: a name column, as wide as the longest name on the page
    let name_width = node.as_container().map(|_| {
        page.iter()
            .map(|s| s.task.name.chars().count())
            .max()
            .unwrap_or(0)
            .min(NAME_MAX)
    });
    let mut lines: Vec<Line> = page
        .iter()
        .map(|s| {
            let row = session_row(s, name_width, info.now);
            // the list cursor: reversed, like the tree cursor
            if list.selected == Some(s.id) {
                row.reversed()
            } else {
                row
            }
        })
        .collect();

    let pages = list.pages(newest_first.len());
    if pages > 1 {
        // a short last page: blank rows, so the page line stays in place
        lines.resize(list.page_len.max(lines.len()), Line::default());
        let current = list.page.min(pages - 1) + 1; // same clamp as `range`
        lines.push(
            Line::from(format!("page {current}/{pages}"))
                .dim()
                .right_aligned(),
        );
    }
    lines
}

/// `lab 3   Thu 15.10  14:00–15:12   1h12  edited`; running: `–now` + `▶`.
fn session_row(s: &Session, name_width: Option<usize>, now: Time) -> Line<'static> {
    let start = s.start.with_timezone(&Local);
    let end = s
        .end
        .map_or_else(|| "now".to_string(), |e| e.with_timezone(&Local).format("%H:%M").to_string());
    let mut spans = Vec::new();
    if let Some(w) = name_width {
        spans.push(Span::raw(format!("{:<w$}  ", fit(&s.task.name, w))));
    }
    spans.push(Span::raw(format!(
        "{}  {}–{end:<5}  {:>5}",
        start.format("%a"),
        start.format(DATE_FMT),
        s.duration(now).to_string(),
    )));
    if s.end.is_none() {
        spans.push(Span::raw("  ▶").green());
    }
    if s.edited_at.is_some() {
        spans.push(Span::raw("  edited").dim());
    }
    if s.source != SessionSource::Manual {
        spans.push(Span::raw(format!("  {}", s.source)).dim());
    }
    Line::from(spans)
}

/// `name` cut to `width` characters, the last one `…` if it was longer.
fn fit(name: &str, width: usize) -> String {
    if name.chars().count() <= width {
        return name.to_string();
    }
    let cut: String = name.chars().take(width.saturating_sub(1)).collect();
    format!("{cut}…")
}
