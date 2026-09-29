use ratatui::{
    Terminal,
    backend::TestBackend,
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier},
};

use std::path::PathBuf;

use chrono::{Local, TimeDelta};
use crossterm::event::KeyCode;

use super::*;
use crate::{
    model::{
        id::NodeId,
        node::{Node, NodeBody},
        sessions::SessionPatch,
        settings::{ContainerSettings, view::SETTINGS},
        task::Task,
        time::{DeadlineRule, Minutes, Time},
        tree::{PurgePlan, Tree},
    },
    test_util::{at, container, press, state_at, task, test_app, tree_with},
    tui::{
        app::{Confirm, ConfirmStage, PurgeOption, details::DetailsTab},
        form::{Form, TextInput},
        keys::{KEYMAP, bindings},
        toast::Toast,
        tree_state::TreeState,
    },
};

/// Render one frame of `app` into a fake 80x24 terminal (24 rows: the
/// help overlay must fit). One String per screen row, built from cells,
/// so column indexes are real screen columns.
fn render_rows(app: &mut App) -> Vec<String> {
    render_rows_sized(app, 80, 24)
}

/// Like `render_rows`, on a `width` x `height` terminal.
fn render_rows_sized(app: &mut App, width: u16, height: u16) -> Vec<String> {
    rows_of(&render_buffer(app, width, height))
}

/// One frame of `app` on a `width` x `height` terminal, drawn at
/// `at(12, 0)`.
fn render_buffer(app: &mut App, width: u16, height: u16) -> Buffer {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|f| draw(f, app, at(12, 0))).unwrap();
    terminal.backend().buffer().clone()
}

fn rows_of(buf: &Buffer) -> Vec<String> {
    (0..buf.area.height)
        .map(|y| (0..buf.area.width).map(|x| buf[(x, y)].symbol()).collect())
        .collect()
}

/// Whole screen as one string, for simple "is it there" checks.
fn render(app: &mut App) -> String {
    render_rows(app).concat()
}

/// (row, column) of the first occurrence of `needle` on screen.
fn find(rows: &[String], needle: &str) -> Option<(usize, usize)> {
    rows.iter().enumerate().find_map(|(y, row)| {
        let byte = row.find(needle)?;
        Some((y, row[..byte].chars().count()))
    })
}

fn empty_tree() -> Tree {
    tree_with(vec![])
}

#[test]
fn renders_rows_details_and_hint() {
    let t = tree_with(vec![container("uni", vec![task("exam")])]);
    let screen = render(&mut test_app(t, state_at(&[0, 0])));
    assert!(screen.contains("▾ uni/"));
    assert!(screen.contains("○ exam"));
    assert!(screen.contains("to do")); // details pane of the selected task
    assert!(screen.contains("? help"));
}

// ---------- status + overdue (rendered at `at(12, 0)`) ----------

/// Task `name` due at `due`, done at `done_at`.
fn task_due(name: &str, due: Time, done_at: Option<Time>) -> Node {
    let mut t = Task::new(None, due);
    t.done_at = done_at;
    Node::task(name.into(), t)
}

/// Foreground color of the first cell of `needle` on screen.
fn fg_of(app: &mut App, needle: &str) -> Color {
    let buf = render_buffer(app, 80, 24);
    let (y, x) = find(&rows_of(&buf), needle).unwrap_or_else(|| panic!("{needle:?} not on screen"));
    buf[(x as u16, y as u16)].fg
}

#[test]
fn overdue_name_is_red_unless_done() {
    let t = tree_with(vec![
        task_due("late", at(11, 0), None),
        task_due("finished", at(11, 0), Some(at(10, 0))),
        task_due("later", at(13, 0), None),
    ]);
    let mut app = test_app(t, state_at(&[]));

    assert_eq!(fg_of(&mut app, "late"), Color::Red);
    assert_ne!(fg_of(&mut app, "finished"), Color::Red);
    assert_ne!(fg_of(&mut app, "later"), Color::Red);
    let screen = render(&mut app);
    assert!(screen.contains("● finished") && screen.contains("○ late"));
}

#[test]
fn details_due_line_says_overdue() {
    let t = tree_with(vec![
        task_due("late", at(11, 0), None),
        task_due("later", at(13, 0), None),
    ]);

    let mut app = test_app(t, state_at(&[0]));

    let late = render(&mut app);
    app.tree_state.cursor = vec![1];
    let later = render(&mut app);

    assert!(late.contains(" (overdue)"));
    assert!(!later.contains("overdue"));
}

#[tokio::test]
async fn a_session_through_core_makes_the_task_started() {
    let t = tree_with(vec![task("exam")]);
    let mut app = test_app(t, state_at(&[0]));
    app.core
        .add_session(&[0], at(9, 0), at(10, 0), at(20, 0))
        .await
        .unwrap();

    app.reload().await;

    let screen = render(&mut app);
    assert!(screen.contains("◐ exam"), "{screen}");
    assert!(screen.contains("started"));
}

// ---------- timer ----------

/// root: [exam, other], the timer on "exam" since 10:48 (1h12 at the
/// render time 12:00).
async fn timed_app() -> App<'static> {
    let mut app = test_app(tree_with(vec![task("exam"), task("other")]), state_at(&[]));
    app.core.start(&[0], at(10, 48)).await.unwrap();
    app.reload().await;
    app
}

#[tokio::test]
async fn bottom_line_shows_the_running_timer_and_the_hint() {
    let mut app = timed_app().await;

    let rows = render_rows(&mut app);

    let bottom = &rows[23];
    assert!(bottom.starts_with(" ▶ exam · 1h12"), "{bottom}");
    assert!(bottom.trim_end().ends_with("q quit"), "{bottom}");
}

#[test]
fn idle_bottom_line_is_only_the_hint() {
    let mut app = test_app(tree_with(vec![task("exam")]), state_at(&[]));

    let rows = render_rows(&mut app);

    let bottom = &rows[23];
    assert!(!bottom.contains('▶'), "{bottom}");
    assert!(bottom.trim_end().ends_with("? help · q quit"), "{bottom}");
}

/// The timed row keeps its status icon; only the name is bold and green.
#[tokio::test]
async fn the_timed_task_is_bold_and_green_in_the_tree() {
    let mut app = timed_app().await;

    let buf = render_buffer(&mut app, 80, 24);
    let rows = rows_of(&buf);

    assert!(rows.concat().contains("◐ exam"), "{}", rows.join("\n"));
    let (y, x) = find(&rows, "exam").unwrap(); // the tree row comes first
    let cell = &buf[(x as u16, y as u16)];
    assert_eq!(cell.fg, Color::Green);
    assert!(cell.modifier.contains(Modifier::BOLD));
    assert_ne!(fg_of(&mut app, "other"), Color::Green);
}

// ---------- details: time rows (rendered at `at(12, 0)`) ----------

/// root: [uni: [lab, sheet]], uni sets `estimate` to `estimate`.
fn estimate_tree(estimate: Option<u32>) -> Tree {
    let mut t = tree_with(vec![container("uni", vec![task("lab"), task("sheet")])]);
    let uni = t.get_mut(&[0]).and_then(Node::as_container_mut).unwrap();
    uni.settings.estimate = estimate.map(Minutes::new);
    t
}

/// App on `tree` with the cursor on `cursor`, sessions added through
/// `Core` (path, from, to) and reloaded.
async fn app_with_sessions(
    tree: Tree,
    cursor: &[usize],
    sessions: &[(&[usize], Time, Time)],
) -> App<'static> {
    let mut app = test_app(tree, state_at(cursor));
    for (path, from, to) in sessions {
        app.core
            .add_session(path, *from, *to, at(20, 0))
            .await
            .unwrap();
    }
    app.reload().await;
    app
}

/// The screen row of the details field `label` (labels are padded to
/// `LABEL_WIDTH`), trimmed; None: no such row.
fn field_row(rows: &[String], label: &str) -> Option<String> {
    let padded = format!("{label:<LABEL_WIDTH$}");
    let row = rows.iter().find(|r| r.contains(&padded))?;
    let value = &row[row.find(&padded).unwrap() + padded.len()..];
    Some(value.trim_end_matches(['│', ' ']).to_string())
}

#[tokio::test]
async fn task_time_rows_show_estimate_duration_and_left() {
    let sessions: &[(&[usize], _, _)] = &[
        (&[0, 0], at(9, 0), at(9, 30)),
        (&[0, 0], at(10, 0), at(10, 42)),
    ];
    let mut app = app_with_sessions(estimate_tree(Some(120)), &[0, 0], sessions).await;

    let rows = render_rows(&mut app);

    assert_eq!(field_row(&rows, "estimate").as_deref(), Some("2h (from uni)"));
    assert_eq!(field_row(&rows, "duration").as_deref(), Some("1h12 in 2 sessions"));
    assert_eq!(field_row(&rows, "left").as_deref(), Some("48m"));
}

#[tokio::test]
async fn one_session_is_singular() {
    let sessions: &[(&[usize], _, _)] = &[(&[0, 0], at(9, 0), at(9, 45))];
    let mut app = app_with_sessions(estimate_tree(None), &[0, 0], sessions).await;

    let rows = render_rows(&mut app);

    assert_eq!(field_row(&rows, "duration").as_deref(), Some("45m in 1 session"));
}

#[tokio::test]
async fn no_estimate_shows_dashes() {
    let mut app = app_with_sessions(estimate_tree(None), &[0, 0], &[]).await;

    let rows = render_rows(&mut app);

    assert_eq!(field_row(&rows, "estimate").as_deref(), Some("-"));
    assert_eq!(field_row(&rows, "duration").as_deref(), Some("0m in 0 sessions"));
    assert_eq!(field_row(&rows, "left").as_deref(), Some("-"));
}

#[tokio::test]
async fn over_the_estimate_is_red() {
    let sessions: &[(&[usize], _, _)] = &[(&[0, 0], at(9, 0), at(10, 20))];
    let mut app = app_with_sessions(estimate_tree(Some(60)), &[0, 0], sessions).await;

    let rows = render_rows(&mut app);

    assert_eq!(field_row(&rows, "left").as_deref(), Some("over by 20m"));
    assert_eq!(fg_of(&mut app, "over by"), Color::Red);
}

#[tokio::test]
async fn a_running_timer_says_so() {
    let mut app = app_with_sessions(estimate_tree(Some(120)), &[0, 0], &[]).await;
    app.core.start(&[0, 0], at(11, 30)).await.unwrap();
    app.reload().await;

    let rows = render_rows(&mut app);

    let duration = field_row(&rows, "duration").unwrap();
    assert!(duration.ends_with(", running"), "{duration}");
    assert!(duration.starts_with("30m"), "{duration}");
}

#[tokio::test]
async fn a_done_task_has_no_left_row() {
    let mut t = estimate_tree(Some(120));
    let NodeBody::Task(lab) = &mut t.get_mut(&[0, 0]).unwrap().body else {
        panic!("lab is a task");
    };
    lab.done_at = Some(at(11, 0));
    let mut app = app_with_sessions(t, &[0, 0], &[]).await;

    let rows = render_rows(&mut app);

    assert!(field_row(&rows, "estimate").is_some());
    assert_eq!(field_row(&rows, "left"), None);
}

#[tokio::test]
async fn a_container_shows_only_the_duration_of_the_tasks_below() {
    let sessions: &[(&[usize], _, _)] = &[
        (&[0, 0], at(9, 0), at(10, 0)),
        (&[0, 1], at(10, 0), at(10, 30)),
    ];
    let mut app = app_with_sessions(estimate_tree(Some(120)), &[0], sessions).await;

    let rows = render_rows(&mut app);

    assert_eq!(field_row(&rows, "duration").as_deref(), Some("1h30 in 2 sessions"));
    assert_eq!(field_row(&rows, "estimate"), None);
    assert_eq!(field_row(&rows, "left"), None);
}

// ---------- details tabs ----------

/// root: [uni: [cs: [lab]]], uni sets the deadline, cursor on task "lab".
fn tabs_tree() -> Tree {
    let mut t = tree_with(vec![container("uni", vec![container("cs", vec![task("lab")])])]);
    let uni = t.get_mut(&[0]).and_then(Node::as_container_mut).unwrap();
    uni.settings.default_deadline = Some("fri 22:00".parse().unwrap());
    t
}

#[test]
fn details_title_names_every_tab() {
    let t = tabs_tree();
    let screen = render(&mut test_app(t, state_at(&[0, 0, 0])));
    for tab in DetailsTab::ALL {
        assert!(screen.contains(tab.title()), "{} missing", tab.title());
    }
}

#[test]
fn info_tab_is_the_default() {
    let t = tabs_tree();
    let screen = render(&mut test_app(t, state_at(&[0, 0, 0])));
    assert!(screen.contains("to do")); // task status: info tab
    assert!(!screen.contains("fri 22:00")); // no settings
}

#[test]
fn settings_tab_shows_values_and_sources() {
    let t = tabs_tree();
    let mut app = test_app(t, state_at(&[0, 0, 0]));
    app.details_tab = DetailsTab::Settings;

    let rows = render_rows(&mut app);
    let screen = rows.concat();

    assert!(screen.contains("fri 22:00 (from uni)"), "got:\n{}", rows.join("\n"));
    assert!(screen.contains("none (default)")); // task folders
    assert!(!screen.contains("to do")); // no info lines
    let archive = rows
        .iter()
        .find(|r| r.contains("archive"))
        .expect("no archive row");
    assert!(archive.contains('-'), "unset archive should show -: {archive}");
}

/// Settings form of "uni" open, with the field of `SETTINGS` entry `key`
/// active.
fn settings_form_on(app: &mut App, key: &str) {
    let mut form =
        Form::edit_settings(vec![0], "uni", &ContainerSettings::default(), |_| String::new(), None);
    form.active_field = SETTINGS.iter().position(|i| i.key == key).unwrap();
    app.mode = Mode::Form(Box::new(form));
}

// ---------- sessions tab: page length ----------

#[test]
fn page_len_leaves_room_for_borders_header_and_page_line() {
    assert_eq!(details::page_len(Rect::new(0, 0, 50, 20)), 15);
    assert_eq!(details::page_len(Rect::new(0, 0, 50, 3)), 1); // never 0
}

/// Screen height minus the bottom line (1), the details borders and header
/// (4) and the page line (1).
#[test]
fn page_len_follows_the_terminal_height() {
    let mut app = test_app(tree_with(vec![task("exam")]), state_at(&[0]));

    render_rows_sized(&mut app, 80, 24);
    assert_eq!(app.session_list.page_len, 24 - 6);

    render_rows_sized(&mut app, 80, 40);
    assert_eq!(app.session_list.page_len, 40 - 6);
}

#[test]
fn an_open_form_leaves_less_room() {
    let mut app = test_app(tabs_tree(), state_at(&[0]));
    render_rows(&mut app);
    let full = app.session_list.page_len;

    settings_form_on(&mut app, "estimate");
    render_rows(&mut app);

    assert!(app.session_list.page_len < full, "{} vs {full}", app.session_list.page_len);
}

#[test]
fn settings_form_shows_the_format_of_the_active_field() {
    let t = tabs_tree();
    let mut app = test_app(t, state_at(&[0]));

    settings_form_on(&mut app, "default_deadline");
    let rows = render_rows(&mut app);
    let expected = format!("e.g. {}", DeadlineRule::EXAMPLES);
    assert!(rows.concat().contains(&expected), "got:\n{}", rows.join("\n"));

    settings_form_on(&mut app, "task_folders"); // a choice: no format
    assert!(!render(&mut app).contains("e.g."));
}

#[test]
fn both_tabs_start_with_the_name() {
    for tab in DetailsTab::ALL {
        let t = tabs_tree();
        let mut app = test_app(t, state_at(&[0, 0])); // "cs"
        app.details_tab = tab;

        let rows = render_rows(&mut app);

        // right pane only (the tree pane on the left also says "cs/"; the
        // right one starts at 35% of 80 columns), without its borders;
        // first line with text = the name
        let right: Vec<String> = rows
            .iter()
            .map(|r| r.chars().skip(28).collect::<String>())
            .map(|r| r.trim_matches(|c| c == '│' || c == ' ').to_string())
            .collect();
        let first = right[1..]
            .iter()
            .find(|r| !r.is_empty())
            .expect("empty pane");
        assert_eq!(first, "cs", "{tab:?}:\n{}", right.join("\n"));
    }
}

#[test]
fn renders_hint_on_empty_tree() {
    let t = empty_tree();
    assert!(render(&mut test_app(t, TreeState::default())).contains("Nothing here yet"));
}

// ---------- tree pane ----------

#[test]
fn folded_container_shows_closed_marker_and_hides_children() {
    let t = tree_with(vec![container("uni", vec![task("exam")])]);
    let mut state = state_at(&[0]);
    state.collapse(&t);

    let screen = render(&mut test_app(t, state));

    assert!(screen.contains("▸ uni/"));
    assert!(!screen.contains("exam"));
}

#[test]
fn list_selection_follows_the_cursor_row() {
    let t = tree_with(vec![task("a"), container("uni", vec![task("exam")])]);
    let mut app = test_app(t, state_at(&[1, 0]));

    render(&mut app);

    assert_eq!(app.tree_state.list.selected(), Some(3)); // root, a, uni, exam
}

#[test]
fn root_row_comes_first_with_its_dir() {
    let t = tree_with(vec![container("uni", vec![task("exam")])]);
    let rows = render_rows(&mut test_app(t, state_at(&[0])));

    let (root_row, _) = find(&rows, "root").expect("no root row");
    let (uni_row, _) = find(&rows, "uni/").expect("no uni row");
    assert!(root_row < uni_row);
    assert!(rows[root_row].contains("/tmp/root"));
    assert!(!rows[root_row].contains('▾')); // the root doesn't fold
}

#[test]
fn root_row_on_an_empty_tree_is_selected_with_a_hint() {
    let t = empty_tree();
    let mut app = test_app(t, TreeState::default());

    let screen = render(&mut app);

    assert!(screen.contains("root"));
    assert_eq!(app.tree_state.list.selected(), Some(0));
}

// ---------- popups ----------

#[test]
fn error_toast_top_right_and_hint_stays() {
    let t = empty_tree();
    let mut app = test_app(t, TreeState::default());
    app.toast = Some(Toast::error("boom"));

    let rows = render_rows(&mut app);

    let (row, col) = find(&rows, "boom").expect("toast missing");
    assert!(row <= 2, "toast not at the top (row {row})");
    assert!(col >= 40, "toast not on the right (col {col})");
    assert!(rows[..4].iter().any(|r| r.contains("error")));
    assert!(rows.last().unwrap().contains("q quit"));
}

#[test]
fn info_toast_top_right() {
    let t = empty_tree();
    let mut app = test_app(t, TreeState::default());
    app.toast = Some(Toast::info("saved"));

    let rows = render_rows(&mut app);

    let (row, col) = find(&rows, "saved").expect("toast missing");
    assert!(row <= 2 && col >= 40);
}

#[test]
fn long_toast_wraps_instead_of_cutting_off() {
    let words = [
        "alpha", "bravo", "charlie", "delta", "echo", "foxtrot", "golf", "hotel", "india",
        "juliett", "kilo", "lima",
    ];
    let t = empty_tree();
    let mut app = test_app(t, TreeState::default());
    app.toast = Some(Toast::error(words.join(" ")));

    let rows = render_rows(&mut app);

    for w in words {
        assert!(find(&rows, w).is_some(), "{w} cut off");
    }
}

#[test]
fn help_overlay_lists_every_binding() {
    let t = empty_tree();
    let mut app = test_app(t, TreeState::default());
    app.mode = Mode::Help;

    let screen = render(&mut app);

    for b in bindings() {
        assert!(screen.contains(b.help), "help for {:?} missing", b.action);
    }
}

#[test]
fn help_overlay_shows_section_headings_in_order() {
    let t = empty_tree();
    let mut app = test_app(t, TreeState::default());
    app.mode = Mode::Help;

    let rows = render_rows(&mut app);

    // (row, col) of each heading; reading order = left column top to
    // bottom, then the right column (the overlay may use two columns)
    // search below the overlay's top border only: the empty-tree hint
    // behind it mentions `udo create-workspace`
    let (top, _) = find(&rows, "keys · any key closes").unwrap();
    let headings: Vec<(usize, usize)> = KEYMAP
        .iter()
        .map(|s| {
            let (r, c) = find(&rows[top..], s.title)
                .unwrap_or_else(|| panic!("heading {:?} missing", s.title));
            (top + r, c)
        })
        .collect();
    let reading_order: Vec<(usize, usize)> = headings.iter().map(|&(r, c)| (c, r)).collect();
    assert!(reading_order.is_sorted(), "headings out of order: {headings:?}");
    // each section's first binding sits right below its heading
    for (s, (row, _)) in KEYMAP.iter().zip(&headings) {
        assert!(
            rows[row + 1].contains(s.bindings[0].help),
            "{:?} not under {:?}",
            s.bindings[0].help,
            s.title
        );
    }
}

#[test]
fn help_overlay_hidden_in_normal_mode() {
    let t = empty_tree();
    assert!(!render(&mut test_app(t, TreeState::default())).contains("toggle this help"));
}

#[test]
fn confirm_popup_names_node_and_keys() {
    let t = tree_with(vec![task("sheet-3")]);
    let mut app = test_app(t, TreeState::default());
    app.mode = Mode::Confirm(Confirm {
        path: vec![0],
        name: "sheet-3".into(),
        purge: PurgeOption::NoFolder,
        stage: ConfirmStage::Ask,
    });

    let screen = render(&mut app);

    assert!(screen.contains("Remove \"sheet-3\" from udo?"));
    assert!(screen.contains("stay on disk"));
    assert!(screen.contains("y remove from udo · n/esc cancel"));
    // only y/n/esc work in the prompt: the help popup's title must not leak in
    assert!(!screen.contains("any key closes"));
}

#[test]
fn confirm_popup_keeps_long_names_visible() {
    let name = "a-really-long-task-name-that-would-not-fit-into-half-the-screen-width";
    let t = tree_with(vec![task(name)]);
    let mut app = test_app(t, TreeState::default());
    app.mode = Mode::Confirm(Confirm {
        path: vec![0],
        name: name.into(),
        purge: PurgeOption::NoFolder,
        stage: ConfirmStage::Ask,
    });

    let screen = render(&mut app);

    assert!(screen.contains(name), "name cut off");
    assert!(screen.contains("from udo?"), "question cut off");
}

/// Plan for node [0] "lab 3" at `dir`, 2 containers / 7 tasks below.
fn purge_plan(dir: &str, outside: Vec<PathBuf>) -> Box<PurgePlan> {
    Box::new(PurgePlan {
        path: vec![0],
        id: NodeId::new(),
        name: "lab 3".into(),
        dir: dir.into(),
        folders: vec![dir.into()],
        outside,
        containers: 2,
        tasks: 7,
    })
}

/// Screen with the prompt for `purge` open in `stage`.
fn render_confirm(purge: PurgeOption, stage: ConfirmStage) -> String {
    let t = tree_with(vec![task("lab 3")]);
    let mut app = test_app(t, TreeState::default());
    app.mode = Mode::Confirm(Confirm {
        path: vec![0],
        name: "lab 3".into(),
        purge,
        stage,
    });
    render(&mut app)
}

fn purge_stage(typed: &str) -> ConfirmStage {
    ConfirmStage::Purge {
        input: TextInput::new(typed),
    }
}

#[test]
fn ask_popup_offers_d_only_when_ready() {
    let ready = PurgeOption::Ready(purge_plan("/x/lab_3", vec![]));
    let screen = render_confirm(ready, ConfirmStage::Ask);
    assert!(screen.contains("D delete with files"));

    let screen = render_confirm(PurgeOption::NoFolder, ConfirmStage::Ask);
    assert!(!screen.contains("D delete"));
    assert!(!screen.contains("not possible"));

    let refused = PurgeOption::Refused("refusing: /x contains your home folder".into());
    let screen = render_confirm(refused, ConfirmStage::Ask);
    assert!(!screen.contains("D delete"));
    assert!(screen.contains("full delete not possible: refusing: /x"));
}

#[test]
fn purge_popup_shows_path_outside_counts_and_hint() {
    let outside: Vec<PathBuf> = (0..7).map(|i| format!("/data/d{i}").into()).collect();
    let plan = purge_plan("/home/me/uni/algo/lab_3", outside);

    let screen = render_confirm(PurgeOption::Ready(plan), purge_stage("/home/me"));

    assert!(screen.contains("delete with files"));
    assert!(screen.contains("delete lab 3 and everything in it?"));
    assert!(screen.contains("folder:  /home/me/uni/algo/lab_3"));
    assert!(screen.contains("also:    /data/d0"));
    for i in 1..5 {
        assert!(screen.contains(&format!("/data/d{i}")), "d{i} missing");
    }
    assert!(!screen.contains("/data/d5"));
    assert!(screen.contains("and 2 more"));
    assert!(screen.contains("contains 2 containers, 7 tasks"));
    assert!(screen.contains("moved to the Trash"));
    assert!(screen.contains("> /home/me"));
    assert!(screen.contains("enter delete · esc cancel"));
}

#[test]
fn purge_popup_keeps_spaces_in_paths() {
    let plan = purge_plan("/x/lab  3 /y", vec![]);
    let screen = render_confirm(PurgeOption::Ready(plan), purge_stage(""));
    assert!(screen.contains("/x/lab  3 /y"), "spaces changed");
}

#[test]
fn purge_popup_wraps_long_paths() {
    // 77 chars: 2 pieces at 80 columns (65 per piece), "/end-marker" whole in the 2nd
    let dir = format!("/start{}/end-marker", "x".repeat(60));
    let plan = purge_plan(&dir, vec![]);
    let screen = render_confirm(PurgeOption::Ready(plan), purge_stage(""));
    assert!(screen.contains("/start"));
    assert!(screen.contains("end-marker"));
}

#[test]
fn help_overlay_uses_blank_lines_when_tall_enough() {
    let t = empty_tree();
    let mut app = test_app(t, TreeState::default());
    app.mode = Mode::Help;

    let rows = render_rows_sized(&mut app, 80, 40);

    // blank line between the last binding of a section and the next heading
    let (row, _) = find(&rows, KEYMAP[1].title).unwrap();
    assert!(
        rows[row - 1]
            .trim_matches(|c| c == '│' || c == ' ')
            .is_empty()
    );
}

#[test]
fn help_overlay_switches_to_two_columns_when_short() {
    let t = empty_tree();
    let mut app = test_app(t, TreeState::default());
    app.mode = Mode::Help;

    let rows = render_rows_sized(&mut app, 80, 20);
    let screen = rows.concat();

    for b in bindings() {
        assert!(screen.contains(b.help), "help for {:?} missing", b.action);
    }
    // some heading sits right of another one -> two columns. Search
    // inside the overlay only (the hint behind it says "create")
    let (top, _) = find(&rows, "keys · any key closes").unwrap();
    let cols: Vec<usize> = KEYMAP
        .iter()
        .map(|s| {
            find(&rows[top..], s.title)
                .unwrap_or_else(|| panic!("heading {:?} missing", s.title))
                .1
        })
        .collect();
    assert!(cols.iter().any(|&c| c != cols[0]), "still one column: {cols:?}");
}

// ---------- sessions tab: rows ----------

/// App on the sessions tab with the cursor on `cursor`, `n` sessions on
/// "lab" ([0, 0]) of `estimate_tree` (root: [uni: [lab, sheet]]): 10
/// minutes each, 20 minutes apart, from midnight.
async fn sessions_tab(cursor: &[usize], n: i64) -> App<'static> {
    let mut app = test_app(estimate_tree(None), state_at(cursor));
    for i in 0..n {
        let start = at(0, 0) + TimeDelta::minutes(i * 20);
        app.core
            .add_session(&[0, 0], start, start + TimeDelta::minutes(10), at(20, 0))
            .await
            .unwrap();
    }
    app.reload().await;
    app.details_tab = DetailsTab::Sessions;
    app
}

/// The details pane's part of a screen row (between its two borders).
fn pane(row: &str) -> &str {
    row.rsplit('│').nth(1).unwrap_or("")
}

/// Session rows on screen, details pane only (only they have the `–` of a
/// time range).
fn session_rows(rows: &[String]) -> Vec<String> {
    rows.iter()
        .map(|r| pane(r).trim().to_string())
        .filter(|r| r.contains('–'))
        .collect()
}

/// `h:m` of `at(h, m)` in the local zone, as the rows show it.
fn clock(h: u32, m: u32) -> String {
    at(h, m).with_timezone(&Local).format("%H:%M").to_string()
}

#[tokio::test]
async fn sessions_are_newest_first() {
    let mut app = sessions_tab(&[0, 0], 3).await;

    let rows = session_rows(&render_rows(&mut app));

    assert_eq!(rows.len(), 3, "{rows:?}");
    assert!(rows[0].contains(&format!("{}–{}", clock(0, 40), clock(0, 50))), "{rows:?}");
    assert!(rows[2].contains(&format!("{}–{}", clock(0, 0), clock(0, 10))), "{rows:?}");
}

#[tokio::test]
async fn a_running_session_shows_now() {
    let mut app = sessions_tab(&[0, 0], 2).await;
    app.core.start(&[0, 0], at(11, 0)).await.unwrap();
    app.reload().await;

    let rows = session_rows(&render_rows(&mut app));

    assert!(rows[0].contains("–now") && rows[0].ends_with('▶'), "{rows:?}");
    assert!(!rows[1].contains('▶'));
}

#[tokio::test]
async fn an_edited_session_is_marked() {
    let mut app = sessions_tab(&[0, 0], 2).await;
    let oldest = app.sessions[0].id;
    let patch = SessionPatch {
        start: Some(at(0, 5)),
        ..Default::default()
    };
    app.core
        .edit_session(oldest, patch, at(20, 0))
        .await
        .unwrap();
    app.reload().await;

    let rows = session_rows(&render_rows(&mut app));

    assert!(rows[1].ends_with("edited"), "{rows:?}"); // the oldest: last row
    assert!(!rows[0].contains("edited"));
}

#[tokio::test]
async fn a_task_has_no_name_column() {
    let mut app = sessions_tab(&[0, 0], 1).await;

    let rows = session_rows(&render_rows(&mut app));

    // the weekday first, no name (local zone: the weekday of 00:00 +02:00)
    let weekday = at(0, 0).with_timezone(&Local).format("%a").to_string();
    assert!(rows[0].starts_with(&weekday), "{rows:?}");
}

#[tokio::test]
async fn a_container_names_the_task() {
    let mut app = sessions_tab(&[0], 1).await; // "lab" at 00:00-00:10
    app.core
        .add_session(&[0, 1], at(1, 0), at(1, 30), at(20, 0))
        .await
        .unwrap();
    app.reload().await;

    let rows = session_rows(&render_rows(&mut app));

    assert!(rows[0].starts_with("sheet  "), "{rows:?}"); // newest first
    assert!(rows[1].starts_with("lab    "), "{rows:?}"); // padded to "sheet"
}

#[tokio::test]
async fn a_long_task_name_is_cut() {
    let t = tree_with(vec![container("uni", vec![task("a very long task name here")])]);
    let mut app = test_app(t, state_at(&[0]));
    app.core
        .add_session(&[0, 0], at(9, 0), at(10, 0), at(20, 0))
        .await
        .unwrap();
    app.reload().await;
    app.details_tab = DetailsTab::Sessions;

    let rows = session_rows(&render_rows(&mut app));

    assert!(rows[0].starts_with("a very long tas…  "), "{rows:?}"); // 16 columns
}

#[tokio::test]
async fn no_sessions_says_so() {
    let mut app = sessions_tab(&[0, 0], 0).await;

    let screen = render(&mut app);

    assert!(screen.contains("no session recorded yet"), "{screen}");
}

/// Keys, not state set by hand: `e` then `l` show page 2 with its first
/// row selected.
#[tokio::test]
async fn l_shows_the_next_page_with_its_first_row_selected() {
    let mut app = sessions_tab(&[0, 0], 30).await; // 24 rows: 18 per page
    render_rows(&mut app); // sets `page_len` as the keys need it

    app.handle_key(press(KeyCode::Char('e'))).await;
    app.handle_key(press(KeyCode::Char('l'))).await;

    let buf = render_buffer(&mut app, 80, 24);
    let rows = rows_of(&buf);
    assert!(rows.concat().contains("page 2/2"));
    // row 18, newest first = session 11 (oldest first): 11 * 20 minutes
    let (y, x) = find(&rows, &format!("{}–", clock(3, 40))).expect("row 18 not shown");
    assert!(
        buf[(x as u16, y as u16)]
            .modifier
            .contains(Modifier::REVERSED)
    );
}

/// The list cursor's row is reversed, the others are not.
#[tokio::test]
async fn the_selected_session_is_reversed() {
    let mut app = sessions_tab(&[0, 0], 2).await;
    app.session_list.selected = Some(app.sessions[0].id); // the oldest: 00:00

    let buf = render_buffer(&mut app, 80, 24);
    let rows = rows_of(&buf);

    let cell_at = |needle: &str| {
        let (y, x) = find(&rows, needle).unwrap_or_else(|| panic!("{needle:?} not on screen"));
        buf[(x as u16, y as u16)].clone()
    };
    let oldest = cell_at(&format!("{}–", clock(0, 0)));
    let newest = cell_at(&format!("{}–", clock(0, 20)));
    assert!(oldest.modifier.contains(Modifier::REVERSED));
    assert!(!newest.modifier.contains(Modifier::REVERSED));
}

/// The page line is always there, at the same place: `page 1/1` too.
#[tokio::test]
async fn one_page_still_has_its_page_line_at_the_bottom() {
    let mut few = sessions_tab(&[0, 0], 2).await;
    let mut many = sessions_tab(&[0, 0], 30).await;

    let one = find(&render_rows(&mut few), "page 1/1").expect("no page line");
    let two = find(&render_rows(&mut many), "page 1/2").expect("no page line");

    assert_eq!(one, two);
}

#[tokio::test]
async fn many_sessions_are_paged() {
    let mut app = sessions_tab(&[0, 0], 30).await;

    let rows = render_rows(&mut app); // 24 rows: page_len 18

    assert_eq!(session_rows(&rows).len(), app.session_list.page_len);
    assert!(rows.concat().contains("page 1/2"));
}

/// The short last page is filled up: the page line does not move.
#[tokio::test]
async fn the_page_line_stays_at_the_bottom() {
    let mut app = sessions_tab(&[0, 0], 30).await;
    let first = find(&render_rows(&mut app), "page 1/2").unwrap();

    app.session_list.page = 1; // 12 of 18 rows
    let rows = render_rows(&mut app);

    assert_eq!(session_rows(&rows).len(), 12);
    assert_eq!(find(&rows, "page 2/2").unwrap(), first);
}
