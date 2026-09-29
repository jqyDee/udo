use std::path::Path;

use crossterm::event::{KeyCode, KeyEventState, KeyModifiers};

use super::{details::DetailsTab, *};
use crate::{
    model::{
        container::ContainerKind,
        node::Node,
        settings::{TaskFolderSetting, view::SETTINGS},
        task::TaskStatus,
        time::DeadlineRule,
        tree::Tree,
    },
    test_util::{
        at, container, container_at, fake_trash, press, state_at, task, test_app, tree_with,
    },
    tui::{
        form::{FieldId, FieldInput, FolderMode, FormAction, TextInput},
        toast::ToastKind,
    },
};

/// root: [a, ws: [b]]. In memory, never saved.
fn tree() -> Tree {
    tree_with(vec![task("a"), container("ws", vec![task("b")])])
}

fn key(c: char) -> KeyEvent {
    press(KeyCode::Char(c))
}

// ---------- flow + modes ----------

#[tokio::test]
async fn q_quits() {
    let t = tree();
    let mut app = test_app(t, state_at(&[0]));
    assert_eq!(app.handle_key(key('q')).await, Flow::Quit);
}

#[tokio::test]
async fn help_opens_and_any_key_closes_it_without_acting() {
    let t = tree();
    let mut app = test_app(t, state_at(&[0]));

    app.handle_key(key('?')).await;
    assert_eq!(app.mode, Mode::Help);

    // `j` only closes the help, the cursor must not move
    assert_eq!(app.handle_key(key('j')).await, Flow::Continue);
    assert_eq!(app.mode, Mode::Normal);
    assert_eq!(app.tree_state.cursor, vec![0]);
}

#[tokio::test]
async fn q_in_help_closes_help_instead_of_quitting() {
    let t = tree();
    let mut app = test_app(t, state_at(&[0]));
    app.handle_key(key('?')).await;
    assert_eq!(app.handle_key(key('q')).await, Flow::Continue);
    assert_eq!(app.mode, Mode::Normal);
}

#[tokio::test]
async fn key_release_is_ignored() {
    let t = tree();
    let mut app = test_app(t, state_at(&[0]));
    let release = KeyEvent {
        kind: KeyEventKind::Release,
        state: KeyEventState::NONE,
        ..key('j')
    };
    app.handle_key(release).await;
    assert_eq!(app.tree_state.cursor, vec![0]);
}

#[tokio::test]
async fn unknown_key_changes_nothing() {
    let t = tree();
    let mut app = test_app(t, state_at(&[0]));
    assert_eq!(app.handle_key(key('#')).await, Flow::Continue);
    assert_eq!(app.tree_state.cursor, vec![0]);
    assert!(app.toast.is_none());
}

#[test]
fn new_keeps_the_given_cursor() {
    let t = tree();
    let app = test_app(t, TreeState::default());
    assert!(app.tree_state.on_root()); // `TreeState::load` picks the start
}

#[tokio::test]
async fn h_from_the_top_level_reaches_the_root_and_j_leaves_it() {
    let t = tree();
    let mut app = test_app(t, state_at(&[1]));

    app.handle_key(key('h')).await;
    assert!(app.tree_state.on_root());
    app.handle_key(key('j')).await;
    assert_eq!(app.tree_state.cursor, vec![0]);
}

#[tokio::test]
async fn edit_and_delete_refuse_the_root() {
    for k in ['e', 'd'] {
        let t = tree();
        let mut app = test_app(t, TreeState::default()); // root row

        app.handle_key(key(k)).await;

        assert_eq!(app.mode, Mode::Normal, "{k}: something opened");
        let toast = app.toast.as_ref().expect("no toast");
        assert_eq!(toast.kind, ToastKind::Error);
        assert!(toast.msg.contains("root"), "{k}: {}", toast.msg);
    }
}

#[tokio::test]
async fn t_on_the_root_creates_in_the_root() {
    let tmp = tempfile::tempdir().unwrap();
    let mut t = Tree::load_from(tmp.path()).await.unwrap();
    t.create(&[], task("first")).await.unwrap();
    let mut app = test_app(t, TreeState::default()); // root row

    app.handle_key(key('t')).await;
    type_into(&mut app, "second").await;
    app.handle_key(press(KeyCode::Enter)).await;

    assert_eq!(app.core.tree().get(&[1]).unwrap().name(), "second");
    assert_eq!(app.tree_state.cursor, vec![1]); // the new task is selected
}

// ---------- actions ----------

#[tokio::test]
async fn navigation_moves_cursor_without_toast() {
    let t = tree();
    let mut app = test_app(t, state_at(&[0]));
    app.handle_key(key('j')).await;
    assert_eq!(app.tree_state.cursor, vec![1]);
    assert!(app.toast.is_none());
}

#[tokio::test]
async fn folding_hides_rows_without_touching_the_tree() {
    let t = tree();
    let mut app = test_app(t, state_at(&[1])); // "ws"
    app.handle_key(key(' ')).await;
    assert_eq!(app.tree_state.rows(app.core.tree()).len(), 3); // root, a, ws; "b" hidden
    app.handle_key(key('j')).await;
    assert_eq!(app.tree_state.cursor, vec![1]); // nothing below "ws"
}

#[tokio::test]
async fn tab_keys_switch_the_details_tab() {
    let t = tree();
    let mut app = test_app(t, state_at(&[0]));
    assert_eq!(app.details_tab, DetailsTab::Info);

    app.handle_key(press(KeyCode::Tab)).await;
    assert_eq!(app.details_tab, DetailsTab::Settings);
    app.handle_key(press(KeyCode::Tab)).await;
    assert_eq!(app.details_tab, DetailsTab::Sessions);
    app.handle_key(press(KeyCode::Tab)).await;
    assert_eq!(app.details_tab, DetailsTab::Info); // wraps
    app.handle_key(press(KeyCode::BackTab)).await;
    assert_eq!(app.details_tab, DetailsTab::Sessions); // wraps back

    assert_eq!(app.tree_state.cursor, vec![0]); // the tree is untouched
    assert!(app.toast.is_none());
}

#[tokio::test]
async fn tab_in_a_form_moves_between_fields_not_tabs() {
    let t = tree();
    let mut app = test_app(t, state_at(&[0]));
    app.handle_key(key('t')).await;

    app.handle_key(press(KeyCode::Tab)).await;

    assert_eq!(app.details_tab, DetailsTab::Info);
    let Mode::Form(form) = &app.mode else {
        panic!("no form open");
    };
    assert_eq!(form.active_field, 1);
}

#[tokio::test]
async fn x_on_container_shows_error_toast() {
    let t = tree();
    let mut app = test_app(t, state_at(&[1])); // "ws": fails before any save

    assert_eq!(app.handle_key(key('x')).await, Flow::Continue);

    let toast = app.toast.as_ref().expect("no toast");
    assert_eq!(toast.kind, ToastKind::Error);
    assert!(toast.msg.contains("only tasks"), "got: {}", toast.msg);
}

/// An app on a saved tree with one task "sheet" (saving works), cursor on
/// it. Keep the `TempDir` alive.
async fn sheet_app() -> (tempfile::TempDir, App<'static>) {
    let tmp = tempfile::tempdir().unwrap();
    let mut t = Tree::load_from(tmp.path()).await.unwrap();
    let path = t.create(&[], task("sheet")).await.unwrap();
    (tmp, test_app(t, state_at(&path)))
}

fn sheet_status(app: &App) -> TaskStatus {
    let node = app.core.tree().get(&[0]).expect("no sheet");
    let t = node.as_task().expect("expected a task");
    t.status(app.with_sessions.contains(&node.id()))
}

#[tokio::test]
async fn x_marks_done_and_shows_info_toast() {
    let (_tmp, mut app) = sheet_app().await;

    app.handle_key(key('x')).await;

    let toast = app.toast.as_ref().expect("no toast");
    assert_eq!(toast.kind, ToastKind::Info);
    assert_eq!(toast.msg, "sheet -> done");
    assert_eq!(sheet_status(&app), TaskStatus::Done);
}

#[tokio::test]
async fn x_again_reopens_to_do_without_sessions() {
    let (_tmp, mut app) = sheet_app().await;

    app.handle_key(key('x')).await;
    app.handle_key(key('x')).await;

    assert_eq!(app.toast.as_ref().expect("no toast").msg, "sheet -> to do");
    assert_eq!(sheet_status(&app), TaskStatus::ToDo);
}

#[tokio::test]
async fn x_again_reopens_started_with_sessions() {
    let (_tmp, mut app) = sheet_app().await;
    app.core
        .add_session(&[0], at(9, 0), at(10, 0))
        .await
        .unwrap();

    app.handle_key(key('x')).await;
    app.handle_key(key('x')).await;

    assert_eq!(app.toast.as_ref().expect("no toast").msg, "sheet -> started");
    assert_eq!(sheet_status(&app), TaskStatus::Started);
}

// ---------- timer (`s`) ----------
// `s` starts / stops at `time::now()`: assertions avoid exact times.

/// Name of the task the timer runs on, if any.
fn timed(app: &App) -> Option<String> {
    app.running.as_ref().map(|s| s.task.name.clone())
}

fn toast_of(app: &App) -> (ToastKind, String) {
    let toast = app.toast.as_ref().expect("no toast");
    (toast.kind, toast.msg.clone())
}

#[tokio::test]
async fn s_starts_the_timer_on_a_task() {
    let (_tmp, mut app) = sheet_app().await;

    app.handle_key(key('s')).await;

    assert_eq!(timed(&app).as_deref(), Some("sheet"));
    assert_eq!(toast_of(&app), (ToastKind::Info, "▶ sheet".into()));
    assert_eq!(sheet_status(&app), TaskStatus::Started);
}

#[tokio::test]
async fn s_again_stops_it() {
    let (_tmp, mut app) = sheet_app().await;

    app.handle_key(key('s')).await;
    app.handle_key(key('s')).await;

    assert_eq!(timed(&app), None);
    let (kind, msg) = toast_of(&app);
    assert_eq!(kind, ToastKind::Info);
    assert!(msg.starts_with("stopped sheet ("), "got: {msg}");
    assert_eq!(sheet_status(&app), TaskStatus::Started); // the session stays
}

#[tokio::test]
async fn s_on_another_task_switches() {
    let mut app = test_app(tree(), state_at(&[0])); // root: [a, ws: [b]]
    app.handle_key(key('s')).await;

    app.tree_state.cursor = vec![1, 0];
    app.handle_key(key('s')).await;

    assert_eq!(timed(&app).as_deref(), Some("b"));
}

#[tokio::test]
async fn s_on_a_container_or_the_root_is_refused() {
    for cursor in [&[1][..], &[]] {
        let mut app = test_app(tree(), state_at(cursor));

        app.handle_key(key('s')).await;

        assert_eq!(timed(&app), None, "cursor {cursor:?}");
        let expected = (ToastKind::Error, "only tasks can be timed".to_string());
        assert_eq!(toast_of(&app), expected, "cursor {cursor:?}");
    }
}

#[tokio::test]
async fn s_on_a_done_task_is_refused() {
    let (_tmp, mut app) = sheet_app().await;
    app.handle_key(key('x')).await;

    app.handle_key(key('s')).await;

    assert_eq!(timed(&app), None);
    let expected = (ToastKind::Error, "sheet is done: press x to reopen it".to_string());
    assert_eq!(toast_of(&app), expected);
}

/// As the CLI would: straight through `Core`, then the next reload (tick).
#[tokio::test]
async fn a_timer_started_elsewhere_shows_after_reload() {
    let mut app = test_app(tree(), state_at(&[0]));
    app.core.start(&[1, 0], at(9, 0)).await.unwrap();
    assert_eq!(timed(&app), None);

    app.reload().await;

    assert_eq!(timed(&app).as_deref(), Some("b"));
}

#[tokio::test]
async fn old_status_keys_do_nothing() {
    let (_tmp, mut app) = sheet_app().await;

    for c in ['p', 'u'] {
        app.handle_key(key(c)).await;
    }

    assert!(app.toast.is_none());
    assert_eq!(sheet_status(&app), TaskStatus::ToDo);
}

// ---------- delete + confirm ----------
// The in-memory `tree()` is never saved: only answers that don't delete
// (n, esc, ignored keys) use it. `y` saves, so it gets a real tempdir tree.

#[tokio::test]
async fn d_opens_confirm_for_selected_node() {
    let t = tree();
    let mut app = test_app(t, state_at(&[0]));

    assert_eq!(app.handle_key(key('d')).await, Flow::Continue);

    assert_eq!(
        app.mode,
        Mode::Confirm(Confirm {
            path: vec![0],
            name: "a".into(),
            purge: PurgeOption::NoFolder, // task without dir
            stage: ConfirmStage::Ask,
        })
    );
}

#[tokio::test]
async fn n_and_esc_cancel_without_deleting() {
    for cancel in [key('n'), press(KeyCode::Esc)] {
        let t = tree();
        let mut app = test_app(t, state_at(&[0]));
        app.handle_key(key('d')).await;

        app.handle_key(cancel).await;

        assert_eq!(app.mode, Mode::Normal, "{cancel:?} did not close");
        assert_eq!(app.core.tree().get(&[0]).unwrap().name(), "a");
        assert!(app.toast.is_none());
    }
}

#[tokio::test]
async fn other_keys_are_ignored_while_confirming() {
    let t = tree();
    let mut app = test_app(t, state_at(&[0]));
    app.handle_key(key('d')).await;

    // neither moves the cursor behind the prompt nor quits
    assert_eq!(app.handle_key(key('j')).await, Flow::Continue);
    assert_eq!(app.handle_key(key('q')).await, Flow::Continue);

    assert!(matches!(app.mode, Mode::Confirm(_)));
    assert_eq!(app.tree_state.cursor, vec![0]);
}

#[tokio::test]
async fn d_without_selection_shows_error() {
    let t = tree_with(vec![]); // empty tree: cursor stays on the root
    let mut app = test_app(t, TreeState::default());

    app.handle_key(key('d')).await;

    assert_eq!(app.mode, Mode::Normal);
    let toast = app.toast.as_ref().expect("no toast");
    assert_eq!(toast.kind, ToastKind::Error);
}

#[tokio::test]
async fn y_removes_node_from_tree_and_disk() {
    let tmp = tempfile::tempdir().unwrap();
    let mut t = Tree::load_from(tmp.path()).await.unwrap(); // real root: saving works
    let path = t.create(&[], task("sheet")).await.unwrap();
    let mut app = test_app(t, state_at(&path));

    app.handle_key(key('d')).await;
    app.handle_key(key('y')).await;

    assert_eq!(app.mode, Mode::Normal);
    assert!(app.core.tree().get(&path).is_none());
    assert!(app.tree_state.cursor.is_empty()); // last node gone: nothing selected
    let toast = app.toast.as_ref().expect("no toast");
    assert_eq!(toast.kind, ToastKind::Info);
    assert!(toast.msg.contains("removed sheet"), "got: {}", toast.msg);

    // saved: a fresh load doesn't have it either
    let reloaded = Tree::load_from(tmp.path()).await.unwrap();
    assert!(reloaded.rows().is_empty());
}

// ---------- full delete (D) ----------
// Every test that can reach `purge` sets `app.trash = fake_trash`.

/// `D` as terminals send it: with SHIFT.
fn shift_d() -> KeyEvent {
    KeyEvent::new(KeyCode::Char('D'), KeyModifiers::SHIFT)
}

/// Real root (tmp/udo) with project "lab" (tmp/lab) holding task
/// "notes"; select it with `state_at(&[0])`.
async fn lab_tree(tmp: &Path) -> Tree {
    let mut t = Tree::load_from(&tmp.join("udo")).await.unwrap();
    let lab = container_at("lab", &tmp.join("lab"), ContainerKind::Project, vec![]);
    t.create(&[], lab).await.unwrap();
    t.create(&[0], task("notes")).await.unwrap();
    t
}

fn stage<'m>(app: &'m App<'_>) -> &'m ConfirmStage {
    match &app.mode {
        Mode::Confirm(c) => &c.stage,
        other => panic!("no prompt open: {other:?}"),
    }
}

#[tokio::test]
async fn shift_d_with_a_plan_opens_the_purge_stage() {
    let tmp = tempfile::tempdir().unwrap();
    let t = lab_tree(tmp.path()).await;
    let mut app = test_app(t, state_at(&[0]));

    app.handle_key(key('d')).await;
    app.handle_key(shift_d()).await;

    let empty = ConfirmStage::Purge {
        input: TextInput::new(""),
    };
    assert_eq!(stage(&app), &empty);
    assert!(app.toast.is_none());
}

#[tokio::test]
async fn d_on_task_without_folder_shows_hint() {
    let t = tree();
    let mut app = test_app(t, state_at(&[0]));

    app.handle_key(key('d')).await;
    app.handle_key(shift_d()).await;

    assert_eq!(stage(&app), &ConfirmStage::Ask);
    let toast = app.toast.as_ref().expect("no toast");
    assert_eq!(toast.kind, ToastKind::Info);
    assert_eq!(toast.msg, "no folder to delete, use y");
}

#[tokio::test]
async fn d_when_refused_shows_the_reason() {
    let tmp = tempfile::tempdir().unwrap();
    let mut t = Tree::load_from(&tmp.path().join("udo")).await.unwrap();
    // folder above the udo root: never offered (in memory, nothing saved)
    let big = container_at("big", tmp.path(), ContainerKind::Workspace, vec![]);
    t.insert(&[], big).unwrap();
    let mut app = test_app(t, state_at(&[0]));

    app.handle_key(key('d')).await;
    app.handle_key(shift_d()).await;

    assert_eq!(stage(&app), &ConfirmStage::Ask);
    let toast = app.toast.as_ref().expect("no toast");
    assert_eq!(toast.kind, ToastKind::Error);
    assert!(toast.msg.contains("udo root"), "got: {}", toast.msg);
}

#[tokio::test]
async fn wrong_path_keeps_popup_and_files() {
    let tmp = tempfile::tempdir().unwrap();
    let t = lab_tree(tmp.path()).await;
    let lab = tmp.path().join("lab");
    let mut app = test_app(t, state_at(&[0]));
    app.trash = fake_trash;

    let exact = lab.display().to_string();
    let wrongs = [
        "nope yq".to_string(),
        "~/lab".into(),
        format!("{exact}/"),
        format!(" {exact}"),
    ];
    for wrong in wrongs {
        app.handle_key(key('d')).await;
        app.handle_key(shift_d()).await;
        type_into(&mut app, &wrong).await; // y / n / q are text here
        app.handle_key(press(KeyCode::Enter)).await;

        let typed = ConfirmStage::Purge {
            input: TextInput::new(wrong.as_str()),
        };
        assert_eq!(stage(&app), &typed, "{wrong:?}");
        let toast = app.toast.as_ref().expect("no toast");
        assert_eq!(toast.kind, ToastKind::Error);
        assert_eq!(toast.msg, "path does not match");
        assert!(lab.exists());
        assert_eq!(app.core.tree().get(&[0]).unwrap().name(), "lab");
        app.handle_key(press(KeyCode::Esc)).await; // next round from Normal
    }
}

#[tokio::test]
async fn exact_path_deletes_node_and_folder() {
    let tmp = tempfile::tempdir().unwrap();
    let t = lab_tree(tmp.path()).await;
    let lab = tmp.path().join("lab");
    let mut app = test_app(t, state_at(&[0]));
    app.trash = fake_trash;

    app.handle_key(key('d')).await;
    app.handle_key(shift_d()).await;
    type_into(&mut app, &lab.display().to_string()).await;
    app.handle_key(press(KeyCode::Enter)).await;

    assert_eq!(app.mode, Mode::Normal);
    assert!(!lab.exists());
    assert!(app.core.tree().get(&[0]).is_none());
    assert!(app.tree_state.cursor.is_empty()); // last node gone: nothing selected
    let toast = app.toast.as_ref().expect("no toast");
    assert_eq!(toast.kind, ToastKind::Info);
    assert_eq!(toast.msg, "deleted lab · 1 folder moved to Trash");
    let reloaded = Tree::load_from(&tmp.path().join("udo")).await.unwrap();
    assert!(reloaded.rows().is_empty());
}

#[tokio::test]
async fn esc_in_purge_stage_cancels_everything() {
    let tmp = tempfile::tempdir().unwrap();
    let t = lab_tree(tmp.path()).await;
    let mut app = test_app(t, state_at(&[0]));
    app.trash = fake_trash;

    app.handle_key(key('d')).await;
    app.handle_key(shift_d()).await;
    type_into(&mut app, "x").await;
    app.handle_key(press(KeyCode::Esc)).await;

    assert_eq!(app.mode, Mode::Normal); // closed, not back to Ask
    assert!(tmp.path().join("lab").exists());
    assert_eq!(app.core.tree().get(&[0]).unwrap().name(), "lab");
    assert!(app.toast.is_none());
}

// ---------- toast lifetime ----------

#[test]
fn toast_deadline_and_expiry() {
    let t = tree();
    let mut app = test_app(t, state_at(&[0]));
    assert_eq!(app.toast_deadline(), None);

    app.toast = Some(Toast::info("hi"));
    let until = app.toast_deadline().unwrap();

    app.expire_toast(until - std::time::Duration::from_millis(1));
    assert!(app.toast.is_some());
    app.expire_toast(until);
    assert!(app.toast.is_none());
}

// ---------- create forms ----------

fn type_str(s: &str) -> Vec<KeyEvent> {
    s.chars().map(key).collect()
}

#[tokio::test]
async fn task_form_starts_in_the_inherited_folder_mode() {
    for (setting, mode) in [
        (TaskFolderSetting::Auto, FolderMode::Auto),
        (TaskFolderSetting::None, FolderMode::None),
    ] {
        let mut t = tree(); // in memory: the form is only opened, never saved
        let root = t.get_mut(&[]).and_then(Node::as_container_mut).unwrap();
        root.settings.task_folders = Some(setting);
        let mut app = test_app(t, state_at(&[1])); // "ws": inherits from the root

        app.handle_key(key('t')).await;

        let Mode::Form(form) = &app.mode else {
            panic!("no form open");
        };
        assert_eq!(form.folder_mode(), Some(mode), "{setting:?}");
    }
}

#[tokio::test]
async fn task_form_due_date_comes_from_the_deadline_setting() {
    let mut t = tree();
    let root = t.get_mut(&[]).and_then(Node::as_container_mut).unwrap();
    root.settings.default_deadline = Some("+3d 18:00".parse().unwrap());
    let before = chrono::Local::now().date_naive();
    let mut app = test_app(t, state_at(&[1])); // "ws": inherits from the root

    app.handle_key(key('t')).await;

    let Mode::Form(form) = &app.mode else {
        panic!("no form open");
    };
    let due = form.date_value(FieldId::Due).expect("no due field");
    assert_eq!(due.time(), chrono::NaiveTime::from_hms_opt(18, 0, 0).unwrap());
    // `now` is read inside: allow for the date changing while the test runs
    let days = (due.date() - before).num_days();
    assert!(days == 3 || days == 4, "due {due} is {days} days away");
}

#[tokio::test]
async fn t_type_enter_creates_task_and_selects_it() {
    let tmp = tempfile::tempdir().unwrap();
    let t = Tree::load_from(tmp.path()).await.unwrap(); // empty root
    let mut app = test_app(t, TreeState::default());

    app.handle_key(key('t')).await;
    assert!(matches!(app.mode, Mode::Form(_)));
    for k in type_str("exam") {
        app.handle_key(k).await;
    }
    app.handle_key(press(KeyCode::Enter)).await;

    assert_eq!(app.mode, Mode::Normal);
    assert_eq!(app.toast.as_ref().unwrap().kind, ToastKind::Info);
    assert_eq!(app.tree_state.cursor, vec![0]);
    assert_eq!(app.core.tree().get(&[0]).unwrap().name(), "exam");
    assert_eq!(app.core.tree().get(&[0]).unwrap().header.description, None); // left empty
}

#[tokio::test]
async fn description_from_form_is_trimmed_and_saved() {
    let tmp = tempfile::tempdir().unwrap();
    let t = Tree::load_from(tmp.path()).await.unwrap(); // empty root
    let mut app = test_app(t, TreeState::default());

    app.handle_key(key('t')).await;
    for k in type_str("exam") {
        app.handle_key(k).await;
    }
    app.handle_key(press(KeyCode::Tab)).await; // -> description
    for k in type_str("  read ch 3 ") {
        app.handle_key(k).await;
    }
    app.handle_key(press(KeyCode::Enter)).await;

    let desc = &app.core.tree().get(&[0]).unwrap().header.description;
    assert_eq!(desc.as_deref(), Some("read ch 3"));
}

#[tokio::test]
async fn invalid_name_keeps_form_open_with_error() {
    let tmp = tempfile::tempdir().unwrap();
    let t = Tree::load_from(tmp.path()).await.unwrap();
    let mut app = test_app(t, TreeState::default());

    app.handle_key(key('t')).await;
    for k in type_str("a/b") {
        app.handle_key(k).await;
    }
    app.handle_key(press(KeyCode::Enter)).await;

    assert!(matches!(app.mode, Mode::Form(_)), "form closed on error");
    assert_eq!(app.toast.as_ref().unwrap().kind, ToastKind::Error);
    assert!(app.core.tree().get(&[]).unwrap().children().is_empty());
}

#[tokio::test]
async fn esc_closes_form_without_creating() {
    let t = tree();
    let mut app = test_app(t, state_at(&[0]));

    app.handle_key(key('c')).await;
    app.handle_key(key('x')).await;
    app.handle_key(press(KeyCode::Esc)).await;

    assert_eq!(app.mode, Mode::Normal);
    assert_eq!(app.core.tree().get(&[]).unwrap().children().len(), 2);
}

// ---------- edit form ----------

/// Ctrl+U: clears the text field (cursor starts at the end).
fn clear() -> KeyEvent {
    KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL)
}

/// Real root with tasks "sheet" ([0]) and "exam" ([1]).
async fn disk_tree(tmp: &std::path::Path) -> Tree {
    let mut t = Tree::load_from(tmp).await.unwrap();
    t.create(&[], task("sheet")).await.unwrap();
    t.create(&[], task("exam")).await.unwrap();
    t
}

async fn type_into(app: &mut App<'_>, s: &str) {
    for k in type_str(s) {
        app.handle_key(k).await;
    }
}

#[tokio::test]
async fn e_opens_form_prefilled_with_the_node() {
    let tmp = tempfile::tempdir().unwrap();
    let t = disk_tree(tmp.path()).await;
    let mut app = test_app(t, state_at(&[1]));

    app.handle_key(key('e')).await;

    let Mode::Form(form) = &app.mode else {
        panic!("no form open");
    };
    assert_eq!(form.action, FormAction::EditNode { path: vec![1] });
    assert_eq!(form.values().name, "exam");
}

#[tokio::test]
async fn edit_renames_and_sets_description_on_disk() {
    let tmp = tempfile::tempdir().unwrap();
    let t = disk_tree(tmp.path()).await;
    let mut app = test_app(t, state_at(&[0]));

    app.handle_key(key('e')).await;
    app.handle_key(clear()).await;
    type_into(&mut app, "sheet 2").await;
    app.handle_key(press(KeyCode::Tab)).await; // -> description
    type_into(&mut app, "  ex 1-4 ").await;
    app.handle_key(press(KeyCode::Enter)).await;

    assert_eq!(app.mode, Mode::Normal);
    assert_eq!(app.toast.as_ref().unwrap().kind, ToastKind::Info);
    assert_eq!(app.tree_state.cursor, vec![0]); // same node stays selected
    let reloaded = Tree::load_from(tmp.path()).await.unwrap();
    let node = reloaded.get(&[0]).unwrap();
    assert_eq!(node.name(), "sheet 2");
    assert_eq!(node.header.description.as_deref(), Some("ex 1-4"));
}

#[tokio::test]
async fn clearing_the_description_removes_it() {
    let tmp = tempfile::tempdir().unwrap();
    let mut t = disk_tree(tmp.path()).await;
    t.get_mut(&[0]).unwrap().header.description = Some("old".into());
    let mut app = test_app(t, state_at(&[0]));

    app.handle_key(key('e')).await;
    app.handle_key(press(KeyCode::Tab)).await; // -> description
    app.handle_key(clear()).await;
    app.handle_key(press(KeyCode::Enter)).await;

    assert_eq!(app.core.tree().get(&[0]).unwrap().header.description, None);
}

#[tokio::test]
async fn edit_to_a_siblings_name_keeps_form_open_with_error() {
    let tmp = tempfile::tempdir().unwrap();
    let t = disk_tree(tmp.path()).await;
    let mut app = test_app(t, state_at(&[0]));

    app.handle_key(key('e')).await;
    app.handle_key(clear()).await;
    type_into(&mut app, "exam").await; // [1] is called that
    app.handle_key(press(KeyCode::Enter)).await;

    assert!(matches!(app.mode, Mode::Form(_)), "form closed on error");
    assert_eq!(app.toast.as_ref().unwrap().kind, ToastKind::Error);
    assert_eq!(app.core.tree().get(&[0]).unwrap().name(), "sheet");
}

#[tokio::test]
async fn edit_changes_container_kind_on_disk() {
    let tmp = tempfile::tempdir().unwrap();
    let mut t = Tree::load_from(tmp.path()).await.unwrap();
    let ws_dir = tmp.path().join("uni");
    let uni = container_at("uni", &ws_dir, ContainerKind::Workspace, vec![]);
    t.create(&[], uni).await.unwrap();
    let mut app = test_app(t, state_at(&[0]));

    app.handle_key(key('e')).await;
    app.handle_key(press(KeyCode::Tab)).await; // -> description
    app.handle_key(press(KeyCode::Tab)).await; // -> kind
    app.handle_key(press(KeyCode::Right)).await; // workspace -> project
    app.handle_key(press(KeyCode::Enter)).await;

    let reloaded = Tree::load_from(tmp.path()).await.unwrap();
    let uni = reloaded.get(&[0]).and_then(Node::as_container).unwrap();
    assert_eq!(uni.kind, ContainerKind::Project);
    assert_eq!(uni.dir, ws_dir); // dir untouched
}

#[tokio::test]
async fn e_without_selection_shows_error() {
    let t = tree_with(vec![]); // cursor on the root
    let mut app = test_app(t, TreeState::default());

    app.handle_key(key('e')).await;

    assert_eq!(app.mode, Mode::Normal);
    assert_eq!(app.toast.as_ref().unwrap().kind, ToastKind::Error);
}

#[tokio::test]
async fn esc_closes_edit_form_without_changes() {
    let t = tree(); // in memory: Esc never saves
    let mut app = test_app(t, state_at(&[0]));

    app.handle_key(key('e')).await;
    type_into(&mut app, "zzz").await;
    app.handle_key(press(KeyCode::Esc)).await;

    assert_eq!(app.mode, Mode::Normal);
    assert_eq!(app.core.tree().get(&[0]).unwrap().name(), "a");
}

// ---------- settings form (`e` on the settings tab) ----------

/// Real root with container "uni" ([0]) holding task "lab" ([0, 0]).
async fn uni_tree(tmp: &Path) -> Tree {
    let mut t = Tree::load_from(tmp).await.unwrap();
    let uni = container_at("uni", &tmp.join("uni"), ContainerKind::Workspace, vec![]);
    t.create(&[], uni).await.unwrap();
    t.create(&[0], task("lab")).await.unwrap();
    t
}

/// Settings tab, then `e`.
async fn open_settings(app: &mut App<'_>) {
    app.handle_key(press(KeyCode::Tab)).await;
    app.handle_key(key('e')).await;
}

/// Tab from the first field to the field of `SETTINGS` entry `key`.
async fn focus_setting(app: &mut App<'_>, key: &str) {
    let idx = SETTINGS.iter().position(|i| i.key == key).unwrap();
    for _ in 0..idx {
        app.handle_key(press(KeyCode::Tab)).await;
    }
}

fn open_form<'m>(app: &'m App<'_>) -> &'m Form {
    match &app.mode {
        Mode::Form(form) => form,
        other => panic!("no form open: {other:?}"),
    }
}

fn uni_deadline(t: &Tree) -> Option<DeadlineRule> {
    let uni = t.get(&[0]).and_then(Node::as_container).unwrap();
    uni.settings.default_deadline
}

#[tokio::test]
async fn e_on_the_settings_tab_edits_the_tasks_container() {
    let mut t = tree(); // in memory: root: [a, ws: [b]]
    let root = t.get_mut(&[]).and_then(Node::as_container_mut).unwrap();
    root.settings.default_deadline = Some("fri 22:00".parse().unwrap());
    let mut app = test_app(t, state_at(&[1, 0])); // task "b"

    open_settings(&mut app).await;

    let form = open_form(&app);
    assert_eq!(form.action, FormAction::EditSettings { path: vec![1] });
    let deadline = SETTINGS
        .iter()
        .position(|i| i.key == "default_deadline")
        .unwrap();
    let FieldInput::Text(t) = &form.fields[deadline].input else {
        panic!("deadline is not a text field");
    };
    assert_eq!(t.value, ""); // not set on ws
    assert_eq!(t.placeholder.as_deref(), Some("fri 22:00 (from root)"));
    // not the root: no root settings
    assert!(
        !form
            .fields
            .iter()
            .any(|f| matches!(f.id, FieldId::RootSetting(_)))
    );
}

#[tokio::test]
async fn settings_form_saves_and_keeps_the_cursor() {
    let tmp = tempfile::tempdir().unwrap();
    let t = uni_tree(tmp.path()).await;
    let mut app = test_app(t, state_at(&[0, 0])); // task "lab"

    open_settings(&mut app).await;
    focus_setting(&mut app, "default_deadline").await;
    type_into(&mut app, "fri 22:00").await;
    app.handle_key(press(KeyCode::Enter)).await;

    assert_eq!(app.mode, Mode::Normal);
    assert_eq!(app.toast.as_ref().unwrap().kind, ToastKind::Info);
    assert_eq!(app.tree_state.cursor, vec![0, 0]); // still on the task
    let reloaded = Tree::load_from(tmp.path()).await.unwrap();
    assert_eq!(uni_deadline(&reloaded), Some("fri 22:00".parse().unwrap()));
}

#[tokio::test]
async fn clearing_a_setting_inherits_it_again() {
    let tmp = tempfile::tempdir().unwrap();
    let mut t = uni_tree(tmp.path()).await;
    let uni = t.get_mut(&[0]).and_then(Node::as_container_mut).unwrap();
    uni.settings.default_deadline = Some("fri 22:00".parse().unwrap());
    t.save(&[0]).await.unwrap();
    let mut app = test_app(t, state_at(&[0]));

    open_settings(&mut app).await;
    focus_setting(&mut app, "default_deadline").await;
    app.handle_key(clear()).await;
    app.handle_key(press(KeyCode::Enter)).await;

    assert_eq!(app.mode, Mode::Normal);
    let reloaded = Tree::load_from(tmp.path()).await.unwrap();
    assert_eq!(uni_deadline(&reloaded), None);
}

#[tokio::test]
async fn bad_setting_keeps_the_form_open_with_error() {
    let tmp = tempfile::tempdir().unwrap();
    let t = uni_tree(tmp.path()).await;
    let mut app = test_app(t, state_at(&[0]));

    open_settings(&mut app).await;
    focus_setting(&mut app, "default_deadline").await;
    type_into(&mut app, "someday").await;
    app.handle_key(press(KeyCode::Enter)).await;

    assert!(matches!(app.mode, Mode::Form(_)), "form closed on error");
    let toast = app.toast.as_ref().unwrap();
    assert_eq!(toast.kind, ToastKind::Error);
    assert!(toast.msg.starts_with("deadline: "), "got: {}", toast.msg);
    assert_eq!(uni_deadline(app.core.tree()), None);
}

#[tokio::test]
async fn settings_form_on_the_root_row_saves_root_settings() {
    let tmp = tempfile::tempdir().unwrap();
    let t = uni_tree(tmp.path()).await;
    let mut app = test_app(t, state_at(&[])); // root row

    open_settings(&mut app).await;
    let form = open_form(&app);
    assert_eq!(form.action, FormAction::EditSettings { path: vec![] });
    let theme = form
        .fields
        .iter()
        .position(|f| f.id == FieldId::RootSetting(0))
        .expect("no root settings on the root");
    for _ in 0..theme {
        app.handle_key(press(KeyCode::Tab)).await;
    }
    type_into(&mut app, "dark").await;
    app.handle_key(press(KeyCode::Enter)).await;

    assert_eq!(app.mode, Mode::Normal);
    assert_eq!(app.tree_state.cursor, Vec::<usize>::new());
    let reloaded = Tree::load_from(tmp.path()).await.unwrap();
    assert_eq!(reloaded.root_settings().theme.as_deref(), Some("dark"));
}
