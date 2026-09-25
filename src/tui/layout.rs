//! Drawing: tree list (left), details (right), help hint (bottom),
//! toast (top right) and key help (center) as overlays.

use ratatui::{
    Frame,
    layout::{Constraint, Flex, Layout, Rect},
    style::{Color, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, Clear, List, ListItem, Padding, Paragraph},
};

use crate::{
    model::{
        nav::Row,
        node::Node,
        task::{Task, TaskStatus},
        tree::Tree,
    },
    tui::{
        events::{KEYMAP, key_label},
        state::{Status, UiState},
    },
};

/// Fixed hint; the full key list is the `?` overlay, generated from `KEYMAP`.
const HELP: &str = " ? help · q quit";

pub fn draw(frame: &mut Frame, tree: &Tree, state: &mut UiState) {
    let [main, help] =
        Layout::vertical([Constraint::Fill(1), Constraint::Length(1)]).areas(frame.area());
    let [left, right] =
        Layout::horizontal([Constraint::Percentage(60), Constraint::Percentage(40)]).areas(main);

    // tree
    let rows = tree.rows();
    state
        .list
        .select(rows.iter().position(|r| r.path == tree.cursor));

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
        frame.render_stateful_widget(list, left, &mut state.list);
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

    // overlays last, so they lie on top; help above the toast
    if let Some(status) = &state.status {
        draw_toast(frame, status);
    }
    if state.show_help {
        draw_help(frame);
    }
}

/// Small bordered message box in the top right (green info / red error).
fn draw_toast(frame: &mut Frame, status: &Status) {
    let (msg, title, color) = match status {
        Status::Info(m) => (m, " ✓ ", Color::Green),
        Status::Error(m) => (m, " error ", Color::Red),
    };
    let area = frame.area();
    let max_text_w = (area.width / 2).saturating_sub(4).max(10) as usize;
    let lines = wrap_text(msg, max_text_w);
    let text_w = lines.iter().map(|l| l.chars().count()).max().unwrap_or(0);

    let toast = toast_area(area, text_w as u16 + 4, lines.len() as u16 + 2); // +border +padding
    frame.render_widget(Clear, toast);
    frame.render_widget(
        Paragraph::new(lines.into_iter().map(Line::from).collect::<Vec<_>>()).block(
            Block::bordered()
                .title(title)
                .border_style(Style::new().fg(color))
                .padding(Padding::horizontal(1)),
        ),
        toast,
    );
}

/// `width` x `height` box in the top-right corner of `area`, one cell in
/// from the edges, clamped so it never leaves `area`.
fn toast_area(area: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(area.width);
    let height = height.min(area.height);
    Rect {
        x: area.right().saturating_sub(width + 1).max(area.x),
        y: (area.y + 1).min(area.bottom().saturating_sub(height)),
        width,
        height,
    }
}

/// Greedy word wrap to at most `width` chars per line; longer words are split.
fn wrap_text(text: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut lines = vec![];
    let mut line = String::new();
    for word in text.split_whitespace() {
        let mut word: Vec<char> = word.chars().collect();
        // word too long for any line: cut it into width-sized pieces
        while word.len() > width {
            if !line.is_empty() {
                lines.push(std::mem::take(&mut line));
            }
            lines.push(word.drain(..width).collect());
        }
        if word.is_empty() {
            continue;
        }
        let needed = if line.is_empty() {
            word.len()
        } else {
            line.chars().count() + 1 + word.len()
        };
        if needed > width {
            lines.push(std::mem::take(&mut line));
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.extend(word);
    }
    if !line.is_empty() || lines.is_empty() {
        lines.push(line);
    }
    lines
}

/// Centered key list built from `KEYMAP`.
fn draw_help(frame: &mut Frame) {
    let rows: Vec<(String, &str)> = KEYMAP
        .iter()
        .map(|b| {
            let keys = b.keys.iter().map(key_label).collect::<Vec<_>>().join("/");
            (keys, b.help)
        })
        .collect();
    let key_w = rows
        .iter()
        .map(|(k, _)| k.chars().count())
        .max()
        .unwrap_or(0);
    let help_w = rows
        .iter()
        .map(|(_, h)| h.chars().count())
        .max()
        .unwrap_or(0);

    let lines: Vec<Line> = rows
        .iter()
        .map(|(keys, help)| {
            Line::from(vec![
                Span::raw(format!(" {keys:<key_w$}  ")).bold(),
                Span::raw(*help),
            ])
        })
        .collect();

    // content + 1 leading space + 2 gap + 1 trailing space + 2 borders
    let width = (key_w + help_w + 6) as u16;
    let area = centered(frame.area(), width, lines.len() as u16 + 2);
    frame.render_widget(Clear, area); // wipe what's underneath
    frame.render_widget(
        Paragraph::new(lines).block(Block::bordered().title(" keys · any key closes ")),
        area,
    );
}

/// `width` x `height` rectangle in the middle of `area` (clamped to it).
fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let [area] = Layout::horizontal([Constraint::Length(width)])
        .flex(Flex::Center)
        .areas(area);
    let [area] = Layout::vertical([Constraint::Length(height)])
        .flex(Flex::Center)
        .areas(area);
    area
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
        Line::from(vec![
            Span::raw(format!("{key:<10}")).dim(),
            Span::raw(value),
        ])
    }
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

    /// Render one frame, one String per screen row (cells, not bytes, so
    /// column indexes are real screen columns).
    fn render_rows(tree: &Tree, state: &mut UiState) -> Vec<String> {
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        terminal.draw(|f| draw(f, tree, state)).unwrap();
        let buf = terminal.backend().buffer();
        (0..buf.area.height)
            .map(|y| (0..buf.area.width).map(|x| buf[(x, y)].symbol()).collect())
            .collect()
    }

    /// (row, column) of the first occurrence of `needle` on screen.
    fn find(rows: &[String], needle: &str) -> Option<(usize, usize)> {
        rows.iter().enumerate().find_map(|(y, row)| {
            let byte = row.find(needle)?;
            Some((y, row[..byte].chars().count()))
        })
    }

    fn render_with(tree: &Tree, state: &mut UiState) -> String {
        // 24 rows: the help overlay (one line per binding) must fit
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        terminal.draw(|f| draw(f, tree, state)).unwrap();
        let buf = terminal.backend().buffer().clone();
        buf.content().iter().map(|c| c.symbol()).collect()
    }

    /// Render one frame into a fake 80x12 terminal and return it as text.
    fn render(tree: &Tree) -> String {
        render_with(tree, &mut UiState::default())
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

    #[test]
    fn error_renders() {
        let tree = Tree {
            root: container("root", vec![]),
            cursor: vec![],
        };

        let mut state = UiState::default();
        state.error("boom");
        let rows = render_rows(&tree, &mut state);

        // toast in the top right …
        let (row, col) = find(&rows, "boom").expect("toast missing");
        assert!(row <= 2, "toast not at the top (row {row})");
        assert!(col >= 40, "toast not on the right (col {col})");
        assert!(rows[..4].iter().any(|r| r.contains("error")));
        // … and the help hint stays visible
        assert!(rows.last().unwrap().contains("q quit"));
    }

    #[test]
    fn info_renders() {
        let tree = Tree {
            root: container("root", vec![]),
            cursor: vec![],
        };

        let mut state = UiState::default();
        state.info("saved");
        let rows = render_rows(&tree, &mut state);

        let (row, col) = find(&rows, "saved").expect("toast missing");
        assert!(row <= 2 && col >= 40);
        assert!(rows.last().unwrap().contains("q quit"));
    }

    #[test]
    fn long_toast_wraps_instead_of_cutting_off() {
        let tree = Tree {
            root: container("root", vec![]),
            cursor: vec![],
        };
        let words = [
            "alpha", "bravo", "charlie", "delta", "echo", "foxtrot", "golf", "hotel", "india",
            "juliett", "kilo", "lima",
        ];
        let mut state = UiState::default();
        state.error(words.join(" "));

        let rows = render_rows(&tree, &mut state);

        for w in words {
            assert!(find(&rows, w).is_some(), "{w} cut off");
        }
    }

    #[test]
    fn toast_area_stays_inside_tiny_terminal() {
        let area = Rect::new(0, 0, 10, 3);
        let t = toast_area(area, 40, 8);
        assert!(t.right() <= area.right() && t.bottom() <= area.bottom());
    }

    #[test]
    fn toast_area_is_top_right_with_margin() {
        let t = toast_area(Rect::new(0, 0, 80, 24), 20, 3);
        assert_eq!((t.x, t.y, t.width, t.height), (59, 1, 20, 3));
    }

    #[test]
    fn wrap_text_breaks_on_words() {
        assert_eq!(wrap_text("aa bb cc", 5), vec!["aa bb", "cc"]);
    }

    #[test]
    fn wrap_text_splits_overlong_words() {
        assert_eq!(wrap_text("abcdefgh", 3), vec!["abc", "def", "gh"]);
    }

    #[test]
    fn wrap_text_empty_is_one_empty_line() {
        assert_eq!(wrap_text("", 10), vec![""]);
    }

    #[test]
    fn help_hint_in_bottom_line() {
        let tree = Tree {
            root: container("root", vec![]),
            cursor: vec![],
        };
        assert!(render(&tree).contains("? help"));
    }

    #[test]
    fn help_overlay_lists_every_binding() {
        let tree = Tree {
            root: container("root", vec![]),
            cursor: vec![],
        };
        let mut state = UiState {
            show_help: true,
            ..Default::default()
        };

        let screen = render_with(&tree, &mut state);

        for b in KEYMAP {
            assert!(screen.contains(b.help), "help for {:?} missing", b.action);
        }
    }

    #[test]
    fn help_overlay_hidden_by_default() {
        let tree = Tree {
            root: container("root", vec![]),
            cursor: vec![],
        };
        assert!(!render(&tree).contains("toggle this help"));
    }
}
