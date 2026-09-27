use ratatui::{Terminal, backend::TestBackend};

use std::path::PathBuf;

use super::*;
use crate::{
    model::{
        id::NodeId,
        tree::{PurgePlan, Tree},
    },
    test_util::{container, state_at, task, tree_with},
    tui::{
        app::{Confirm, ConfirmStage, PurgeOption},
        form::TextInput,
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
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|f| draw(f, app)).unwrap();
    let buf = terminal.backend().buffer();
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
    let mut t = tree_with(vec![container("uni", vec![task("exam")])]);
    let screen = render(&mut App::new(&mut t, state_at(&[0, 0])));
    assert!(screen.contains("▾ uni/"));
    assert!(screen.contains("○ exam"));
    assert!(screen.contains("to do")); // details pane of the selected task
    assert!(screen.contains("? help"));
}

#[test]
fn renders_hint_on_empty_tree() {
    let mut t = empty_tree();
    assert!(render(&mut App::new(&mut t, TreeState::default())).contains("Nothing here yet"));
}

// ---------- tree pane ----------

#[test]
fn folded_container_shows_closed_marker_and_hides_children() {
    let mut t = tree_with(vec![container("uni", vec![task("exam")])]);
    let mut state = state_at(&[0]);
    state.collapse(&t);

    let screen = render(&mut App::new(&mut t, state));

    assert!(screen.contains("▸ uni/"));
    assert!(!screen.contains("exam"));
}

#[test]
fn list_selection_follows_the_cursor_row() {
    let mut t = tree_with(vec![task("a"), container("uni", vec![task("exam")])]);
    let mut app = App::new(&mut t, state_at(&[1, 0]));

    render(&mut app);

    assert_eq!(app.tree_state.list.selected(), Some(2)); // a, uni, exam
}

// ---------- popups ----------

#[test]
fn error_toast_top_right_and_hint_stays() {
    let mut t = empty_tree();
    let mut app = App::new(&mut t, TreeState::default());
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
    let mut t = empty_tree();
    let mut app = App::new(&mut t, TreeState::default());
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
    let mut t = empty_tree();
    let mut app = App::new(&mut t, TreeState::default());
    app.toast = Some(Toast::error(words.join(" ")));

    let rows = render_rows(&mut app);

    for w in words {
        assert!(find(&rows, w).is_some(), "{w} cut off");
    }
}

#[test]
fn help_overlay_lists_every_binding() {
    let mut t = empty_tree();
    let mut app = App::new(&mut t, TreeState::default());
    app.mode = Mode::Help;

    let screen = render(&mut app);

    for b in bindings() {
        assert!(screen.contains(b.help), "help for {:?} missing", b.action);
    }
}

#[test]
fn help_overlay_shows_section_headings_in_order() {
    let mut t = empty_tree();
    let mut app = App::new(&mut t, TreeState::default());
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
    assert!(
        reading_order.is_sorted(),
        "headings out of order: {headings:?}"
    );
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
    let mut t = empty_tree();
    assert!(!render(&mut App::new(&mut t, TreeState::default())).contains("toggle this help"));
}

#[test]
fn confirm_popup_names_node_and_keys() {
    let mut t = tree_with(vec![task("sheet-3")]);
    let mut app = App::new(&mut t, TreeState::default());
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
    let mut t = tree_with(vec![task(name)]);
    let mut app = App::new(&mut t, TreeState::default());
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
    let mut t = tree_with(vec![task("lab 3")]);
    let mut app = App::new(&mut t, TreeState::default());
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
    let mut t = empty_tree();
    let mut app = App::new(&mut t, TreeState::default());
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
    let mut t = empty_tree();
    let mut app = App::new(&mut t, TreeState::default());
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
    assert!(
        cols.iter().any(|&c| c != cols[0]),
        "still one column: {cols:?}"
    );
}
