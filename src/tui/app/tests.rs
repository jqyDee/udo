use std::{
    fs,
    os::unix::{fs::PermissionsExt, process::ExitStatusExt},
    path::Path,
    process::ExitStatus,
};

use crossterm::event::{KeyCode, KeyEventState, KeyModifiers};

use super::{details::DetailsTab, *};
use crate::{
    model::{
        container::ContainerKind,
        node::{Node, NodeBody},
        sessions::{Session, SessionError, SessionId},
        settings::{TaskFolderSetting, view::SETTINGS},
        task::TaskStatus,
        time::{DeadlineRule, Time},
        tree::Tree,
    },
    run::{Event, RunError},
    test_util::{
        at, container, container_at, fake_trash, press, run_script, state_at, task, test_app,
        tree_with,
    },
    tui::{
        form::{FieldId, FieldInput, FolderMode, Form, FormAction, TextInput},
        keys::PICKER_KEYMAP,
        pick::{PickAction, PickItem, PickValue, Picker},
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
    assert_eq!(app.mode, Mode::Help(Box::new(Mode::Normal)));

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
        .add_session(&[0], at(9, 0), at(10, 0), at(20, 0))
        .await
        .unwrap();

    app.handle_key(key('x')).await;
    app.handle_key(key('x')).await;

    assert_eq!(
        app.toast.as_ref().expect("no toast").msg,
        "sheet -> started"
    );
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
    let expected = (
        ToastKind::Error,
        "sheet is done: press x to reopen it".to_string(),
    );
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
        // task without dir
        Mode::Confirm(Box::new(Confirm::remove_node(
            vec![0],
            "a".into(),
            PurgeOption::NoFolder
        )))
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
async fn enter_in_the_ask_stage_cancels() {
    let t = tree();
    let mut app = test_app(t, state_at(&[0]));
    app.handle_key(key('d')).await;

    app.handle_key(press(KeyCode::Enter)).await;

    assert_eq!(app.mode, Mode::Normal);
    assert_eq!(app.core.tree().get(&[0]).unwrap().name(), "a");
    assert!(app.toast.is_none());
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

    let empty = ConfirmStage::TypeToConfirm {
        expected: tmp.path().join("lab").display().to_string(),
        input: TextInput::new(""),
    };
    assert_eq!(stage(&app), &empty);
    assert!(app.toast.is_none());
}

#[tokio::test]
async fn shift_d_turns_the_action_into_purge_node() {
    let tmp = tempfile::tempdir().unwrap();
    let t = lab_tree(tmp.path()).await;
    let mut app = test_app(t, state_at(&[0]));

    app.handle_key(key('d')).await;
    app.handle_key(shift_d()).await;

    let Mode::Confirm(c) = &app.mode else {
        panic!("no prompt open: {:?}", app.mode);
    };
    let ConfirmAction::PurgeNode { plan } = &c.action else {
        panic!("not a full delete: {:?}", c.action);
    };
    assert_eq!(plan.path, vec![0]);
    assert_eq!(plan.dir, tmp.path().join("lab"));
}

#[tokio::test]
async fn esc_in_the_purge_stage_keeps_node_and_folder() {
    let tmp = tempfile::tempdir().unwrap();
    let t = lab_tree(tmp.path()).await;
    let mut app = test_app(t, state_at(&[0]));
    app.trash = fake_trash;

    app.handle_key(key('d')).await;
    app.handle_key(shift_d()).await;
    app.handle_key(press(KeyCode::Esc)).await;

    assert_eq!(app.mode, Mode::Normal);
    assert!(tmp.path().join("lab").exists());
    assert_eq!(app.core.tree().get(&[0]).unwrap().name(), "lab");
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

        let typed = ConfirmStage::TypeToConfirm {
            expected: exact.clone(),
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
    assert_eq!(
        due.time(),
        chrono::NaiveTime::from_hms_opt(18, 0, 0).unwrap()
    );
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
    assert_eq!(form.name(), "exam");
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

// ---------- sessions list (`Mode::Sessions`) ----------

/// `tree()` (root: [a, ws: [b]]) with the cursor on "a", `n` sessions on it
/// (10 minutes each, 20 apart, from midnight), the sessions tab shown.
async fn list_app(n: i64) -> App<'static> {
    let mut app = test_app(tree(), state_at(&[0]));
    for i in 0..n {
        let start = at(0, 0) + chrono::TimeDelta::minutes(i * 20);
        app.core
            .add_session(
                &[0],
                start,
                start + chrono::TimeDelta::minutes(10),
                at(20, 0),
            )
            .await
            .unwrap();
    }
    app.reload().await;
    app.details_tab = DetailsTab::Sessions;
    app
}

#[tokio::test]
async fn e_on_the_sessions_tab_enters_the_list_on_the_newest() {
    let mut app = list_app(3).await;

    app.handle_key(key('e')).await;

    assert_eq!(app.mode, Mode::Sessions);
    let newest = app.sessions.last().unwrap().id; // `sessions` is oldest first
    assert_eq!(app.session_list.selected, Some(newest));
}

#[tokio::test]
async fn esc_leaves_the_list() {
    let mut app = list_app(3).await;
    app.handle_key(key('e')).await;

    app.handle_key(press(KeyCode::Esc)).await;

    assert_eq!(app.mode, Mode::Normal);
    assert_eq!(app.session_list.selected, None);
}

// ---------- key help in the list (`?`) ----------

#[tokio::test]
async fn question_mark_in_the_list_opens_help_over_it() {
    let mut app = list_app(3).await;
    app.handle_key(key('e')).await;

    app.handle_key(key('?')).await;

    assert_eq!(app.mode, Mode::Help(Box::new(Mode::Sessions)));
    assert!(app.in_list(), "the tree lit up under the help");
}

/// `j` would move the list cursor: here it only closes the help.
#[tokio::test]
async fn any_key_closes_the_list_help_and_does_nothing_else() {
    let mut app = list_app(3).await;
    app.handle_key(key('e')).await;
    let selected = app.session_list.selected;
    app.handle_key(key('?')).await;

    app.handle_key(key('j')).await;

    assert_eq!(app.mode, Mode::Sessions);
    assert_eq!(app.session_list.selected, selected);
}

/// `q` quits in the list too: in its help it only closes the help.
#[tokio::test]
async fn q_in_the_list_help_closes_it_instead_of_quitting() {
    let mut app = list_app(3).await;
    app.handle_key(key('e')).await;
    app.handle_key(key('?')).await;

    assert_eq!(app.handle_key(key('q')).await, Flow::Continue);
    assert_eq!(app.mode, Mode::Sessions);
}

/// Also from the empty list: back to it, still empty.
#[tokio::test]
async fn help_over_the_empty_list_goes_back_to_it() {
    let mut app = list_app(0).await;
    app.handle_key(key('e')).await;

    app.handle_key(key('?')).await;
    app.handle_key(key('?')).await; // `?` toggles: any key closes

    assert_eq!(app.mode, Mode::Sessions);
    assert_eq!(app.session_list.selected, None);
}

#[tokio::test]
async fn e_without_sessions_enters_the_empty_list() {
    let mut app = list_app(0).await;

    app.handle_key(key('e')).await;

    assert_eq!(app.mode, Mode::Sessions);
    assert_eq!(app.session_list.selected, None);
    assert!(app.toast.is_none(), "{:?}", app.toast);
}

/// Tree keys (done, new task, fold, tab) do nothing in the list.
#[tokio::test]
async fn tree_keys_do_nothing_in_the_list() {
    let mut app = list_app(3).await;
    app.handle_key(key('e')).await;
    let selected = app.session_list.selected;

    // (`j` `k` `h` `l` move, `e` edits, `d` removes, `s` splits, `c` cuts,
    // `q` quits in the list too: see their tests)
    for k in [key('x'), key('t'), key('z'), key(' '), press(KeyCode::Tab)] {
        app.handle_key(k).await;
    }

    assert_eq!(app.mode, Mode::Sessions);
    assert_eq!(app.session_list.selected, selected);
    assert_eq!(app.tree_state.cursor, vec![0]);
    assert_eq!(app.details_tab, DetailsTab::Sessions);
    assert!(app.toast.is_none(), "{:?}", app.toast);
    assert_eq!(app.running, None);
    let a = app.core.tree().get(&[0]).and_then(Node::as_task).unwrap();
    assert_eq!(a.done_at, None);
}

#[tokio::test]
async fn e_on_a_later_page_selects_its_first_row() {
    let mut app = list_app(7).await;
    app.session_list.page_len = 3; // set by drawing; no render here
    app.session_list.page = 1; // rows 3..6, newest first

    app.handle_key(key('e')).await;

    let fourth_newest = app.sessions[7 - 1 - 3].id;
    assert_eq!(app.session_list.selected, Some(fourth_newest));
}

#[tokio::test]
async fn q_quits_from_the_list_too() {
    let mut app = list_app(3).await;
    app.handle_key(key('e')).await;

    assert_eq!(app.handle_key(key('q')).await, Flow::Quit);
}

// ---------- moving in the list (7 sessions, 3 per page) ----------

/// `list_app(7)` in the list, 3 rows per page, on the newest session.
async fn in_list_of_7() -> App<'static> {
    let mut app = list_app(7).await;
    app.session_list.page_len = 3; // set by drawing; no render here
    app.handle_key(key('e')).await;
    app
}

/// Row of the selected session, newest first (0 = newest).
fn selected_row(app: &App) -> usize {
    let selected = app.session_list.selected.expect("nothing selected");
    let oldest_first = app.sessions.iter().position(|s| s.id == selected);
    app.sessions.len() - 1 - oldest_first.expect("selection not in the list")
}

#[tokio::test]
async fn j_and_k_move_the_selection() {
    let mut app = in_list_of_7().await;

    app.handle_key(key('j')).await;
    app.handle_key(key('j')).await;
    assert_eq!(selected_row(&app), 2);
    app.handle_key(key('k')).await;
    assert_eq!(selected_row(&app), 1);
}

#[tokio::test]
async fn j_at_the_end_of_a_page_goes_on_to_the_next() {
    let mut app = in_list_of_7().await;

    for _ in 0..3 {
        app.handle_key(key('j')).await;
    }

    assert_eq!((selected_row(&app), app.session_list.page), (3, 1));
}

#[tokio::test]
async fn l_and_h_turn_the_page_to_its_first_row() {
    let mut app = in_list_of_7().await;
    app.handle_key(key('j')).await; // row 1

    app.handle_key(key('l')).await;
    assert_eq!((selected_row(&app), app.session_list.page), (3, 1));
    app.handle_key(key('l')).await;
    assert_eq!((selected_row(&app), app.session_list.page), (6, 2));
    app.handle_key(key('h')).await;
    assert_eq!((selected_row(&app), app.session_list.page), (3, 1));
}

#[tokio::test]
async fn moving_stops_at_the_ends() {
    let mut app = in_list_of_7().await;

    app.handle_key(key('k')).await; // already the newest
    app.handle_key(key('h')).await; // already the first page
    assert_eq!((selected_row(&app), app.session_list.page), (0, 0));

    for _ in 0..10 {
        app.handle_key(key('j')).await;
    }
    app.handle_key(key('l')).await;
    assert_eq!((selected_row(&app), app.session_list.page), (6, 2));
}

#[tokio::test]
async fn arrow_keys_move_like_hjkl() {
    let mut app = in_list_of_7().await;

    app.handle_key(press(KeyCode::Down)).await;
    assert_eq!(selected_row(&app), 1);
    app.handle_key(press(KeyCode::Right)).await;
    assert_eq!(selected_row(&app), 3);
    app.handle_key(press(KeyCode::Up)).await;
    assert_eq!(selected_row(&app), 2);
    app.handle_key(press(KeyCode::Left)).await;
    assert_eq!(selected_row(&app), 0);
}

// ---------- the list across reloads (changes through `Core`, as elsewhere) ----------

#[tokio::test]
async fn a_session_added_elsewhere_keeps_the_selection() {
    let mut app = in_list_of_7().await;
    app.handle_key(key('j')).await; // row 1
    let selected = app.session_list.selected;

    app.core
        .add_session(&[0], at(9, 0), at(9, 30), at(20, 0))
        .await
        .unwrap(); // newest
    app.reload().await;

    assert_eq!(app.session_list.selected, selected);
    assert_eq!(selected_row(&app), 2); // pushed down by one
}

#[tokio::test]
async fn a_selected_session_removed_elsewhere_moves_to_its_row() {
    let mut app = in_list_of_7().await;
    app.handle_key(key('j')).await; // row 1
    let gone = app.session_list.selected.unwrap();

    app.core.delete_session(gone).await.unwrap();
    app.reload().await;

    assert_eq!(app.mode, Mode::Sessions);
    assert_ne!(app.session_list.selected, Some(gone));
    assert_eq!(selected_row(&app), 1); // the next older one
}

#[tokio::test]
async fn the_last_session_removed_stays_in_the_empty_list() {
    let mut app = list_app(1).await;
    app.handle_key(key('e')).await;

    app.core.delete_session(app.sessions[0].id).await.unwrap();
    app.reload().await;

    assert_eq!(app.mode, Mode::Sessions);
    assert_eq!(app.session_list.selected, None);
    assert!(app.toast.is_none(), "{:?}", app.toast);
}

/// "a" and "b" both have 3 pages; moving the tree cursor to "ws" (b's
/// sessions) starts on page 1, not on a's page 3.
#[tokio::test]
async fn moving_the_tree_cursor_resets_the_page() {
    let mut app = list_app(7).await; // on "a"
    for i in 0..7 {
        let start = at(10, 0) + chrono::TimeDelta::minutes(i * 20);
        app.core
            .add_session(
                &[1, 0],
                start,
                start + chrono::TimeDelta::minutes(10),
                at(20, 0),
            )
            .await
            .unwrap();
    }
    app.session_list.page_len = 3;
    app.session_list.page = 2;

    app.handle_key(key('j')).await; // tree: "a" -> "ws" (reloads itself)

    assert_eq!(app.tree_state.cursor, vec![1]);
    assert_eq!(app.sessions.len(), 7); // b's
    assert_eq!(app.session_list.page, 0);
}

/// Staying on the same node (a tick, any key) keeps the page.
#[tokio::test]
async fn a_reload_on_the_same_node_keeps_the_page() {
    let mut app = list_app(7).await;
    app.session_list.page_len = 3;
    app.session_list.page = 2;

    app.reload().await;

    assert_eq!(app.session_list.page, 2);
}

#[tokio::test]
async fn e_in_the_empty_list_says_no_session_selected() {
    let mut app = list_app(0).await;
    app.handle_key(key('e')).await; // the empty list

    app.handle_key(key('e')).await;

    assert_eq!(app.mode, Mode::Sessions); // no form
    let toast = app.toast.as_ref().expect("no toast");
    assert_eq!(
        (toast.kind, toast.msg.as_str()),
        (ToastKind::Error, "no session selected")
    );
}

// ---------- removing a session (`d` in the list) ----------

#[tokio::test]
async fn d_then_y_removes_the_session_and_selects_its_neighbour() {
    let mut app = list_app(3).await;
    app.handle_key(key('e')).await; // on the newest
    let ids = app.newest_ids();

    app.handle_key(key('d')).await;
    app.handle_key(key('y')).await;

    assert_eq!(app.mode, Mode::Sessions);
    assert_eq!(app.sessions.len(), 2);
    assert_eq!(app.session_list.selected, Some(ids[1])); // the row where it was
    let toast = app.toast.as_ref().expect("no toast");
    assert_eq!(toast.kind, ToastKind::Info);
    assert!(toast.msg.starts_with("removed a: "), "got: {}", toast.msg);
    assert!(toast.msg.ends_with("(10m)"), "got: {}", toast.msg);
}

#[tokio::test]
async fn d_then_n_esc_or_enter_keeps_the_session() {
    for no in [key('n'), press(KeyCode::Esc), press(KeyCode::Enter)] {
        let mut app = list_app(2).await;
        app.handle_key(key('e')).await;
        let selected = app.session_list.selected;

        app.handle_key(key('d')).await;
        app.handle_key(no).await;

        assert_eq!(app.mode, Mode::Sessions, "{no:?}");
        assert_eq!(app.sessions.len(), 2, "{no:?}");
        assert_eq!(app.session_list.selected, selected, "{no:?}");
    }
}

#[tokio::test]
async fn d_on_the_last_session_stays_in_the_empty_list() {
    let mut app = list_app(1).await;
    app.handle_key(key('e')).await;

    app.handle_key(key('d')).await;
    app.handle_key(key('y')).await;

    assert_eq!(app.mode, Mode::Sessions);
    assert!(app.sessions.is_empty());
    assert_eq!(app.session_list.selected, None);
}

#[tokio::test]
async fn d_in_the_empty_list_says_no_session_selected() {
    let mut app = list_app(0).await;
    app.handle_key(key('e')).await; // the empty list

    app.handle_key(key('d')).await;

    assert_eq!(app.mode, Mode::Sessions); // no prompt
    let toast = app.toast.as_ref().expect("no toast");
    assert_eq!(
        (toast.kind, toast.msg.as_str()),
        (ToastKind::Error, "no session selected")
    );
}

#[tokio::test]
async fn removing_the_running_session_says_so_and_stops_the_timer() {
    let mut app = list_app(0).await;
    app.handle_key(key('s')).await; // start the timer on "a"
    app.handle_key(key('e')).await;

    app.handle_key(key('d')).await;
    let Mode::Confirm(c) = &app.mode else {
        panic!("no prompt: {:?}", app.mode);
    };
    assert!(
        c.question.ends_with("? (running: the timer stops)"),
        "{}",
        c.question
    );
    app.handle_key(key('y')).await;

    assert_eq!(app.running, None);
    let toast = app.toast.as_ref().expect("no toast");
    assert!(
        toast.msg.ends_with("–now, timer stopped"),
        "got: {}",
        toast.msg
    );
}

// ---------- the session form (`e` in the list) ----------
// "now" is 20:00 on the test day: the sessions are in the past, tomorrow is
// the future (the real clock would call the whole test day the future).

/// `list_app(n)` with a fixed clock, in the list on the newest session,
/// its form open.
async fn session_form_app(n: i64) -> App<'static> {
    let mut app = list_app(n).await;
    app.clock = || at(20, 0);
    app.handle_key(key('e')).await; // into the list
    app.handle_key(key('e')).await; // the form
    app
}

/// The session `id` as the store has it now.
async fn stored(app: &App<'_>, id: SessionId) -> Session {
    let all = app.core.sessions_of(&[]).await.unwrap();
    all.into_iter().find(|s| s.id == id).expect("session gone")
}

/// The date field's minute segment (it starts on the day).
async fn to_minute_segment(app: &mut App<'_>) {
    app.handle_key(press(KeyCode::Right)).await; // hour
    app.handle_key(press(KeyCode::Right)).await; // minute
}

#[tokio::test]
async fn e_in_the_list_opens_the_session_form() {
    let app = session_form_app(2).await;

    let Mode::Form(form) = &app.mode else {
        panic!("no form open: {:?}", app.mode);
    };
    let id = app.session_list.selected.unwrap();
    assert_eq!(form.action, FormAction::EditSession { id });
}

#[tokio::test]
async fn esc_in_the_session_form_goes_back_to_the_list() {
    let mut app = session_form_app(2).await;
    let selected = app.session_list.selected;

    app.handle_key(press(KeyCode::Esc)).await;

    assert_eq!(app.mode, Mode::Sessions);
    assert_eq!(app.session_list.selected, selected);
}

/// The tree stays dimmed while the list has the keys, also under the
/// session form opened from it; only leaving the list wakes it up.
#[tokio::test]
async fn the_session_form_keeps_the_tree_inactive() {
    let mut app = list_app(2).await;
    assert!(!app.in_list(), "the tree has the keys");

    app.handle_key(key('e')).await; // into the list
    assert!(app.in_list(), "in the list");
    app.handle_key(key('e')).await; // the form
    assert!(app.in_list(), "the form over the list");
    app.handle_key(press(KeyCode::Esc)).await; // back to the list
    assert!(app.in_list(), "back in the list");
    app.handle_key(press(KeyCode::Esc)).await; // back to the tree
    assert!(!app.in_list(), "back in the tree");
}

/// Forms and prompts opened from the tree belong to the tree.
#[tokio::test]
async fn tree_forms_and_prompts_are_not_in_the_list() {
    for open in [key('t'), key('d'), key('?')] {
        let mut app = list_app(1).await;
        app.details_tab = DetailsTab::Info; // `e` here: the node's form

        app.handle_key(open).await;

        assert_ne!(app.mode, Mode::Normal, "{open:?} opened nothing");
        assert!(!app.in_list(), "{open:?}: {:?}", app.mode);
    }
}

/// 00:00-00:10, start one step (1 minute) later: saved, back in the list,
/// the cursor still on it, the new times in the toast.
#[tokio::test]
async fn saving_a_changed_start_moves_the_session() {
    let mut app = session_form_app(1).await;
    let id = app.session_list.selected.unwrap();

    to_minute_segment(&mut app).await;
    app.handle_key(press(KeyCode::Up)).await;
    app.handle_key(press(KeyCode::Enter)).await;

    let s = stored(&app, id).await;
    assert_eq!((s.start, s.end), (at(0, 1), Some(at(0, 10))));
    assert!(s.edited_at.is_some());
    assert_eq!(app.mode, Mode::Sessions);
    assert_eq!(app.session_list.selected, Some(id));
    let toast = app.toast.as_ref().expect("no toast");
    assert_eq!(toast.kind, ToastKind::Info);
    assert!(
        toast.msg.starts_with("a: ") && toast.msg.ends_with("(9m)"),
        "{}",
        toast.msg
    );
}

#[tokio::test]
async fn saving_an_unchanged_form_is_no_edit() {
    let mut app = session_form_app(1).await;
    let id = app.session_list.selected.unwrap();

    app.handle_key(press(KeyCode::Enter)).await;

    assert_eq!(app.mode, Mode::Sessions);
    assert_eq!(stored(&app, id).await.edited_at, None); // no `edited` marker
    assert!(app.toast.is_none(), "{:?}", app.toast);
}

/// The end one day later is tomorrow: refused by `Core`, the form stays
/// open, nothing saved.
#[tokio::test]
async fn a_future_end_is_refused_and_keeps_the_form() {
    let mut app = session_form_app(1).await;
    let id = app.session_list.selected.unwrap();
    let before = stored(&app, id).await;

    app.handle_key(press(KeyCode::Tab)).await; // end, on its day
    app.handle_key(press(KeyCode::Up)).await; // tomorrow
    app.handle_key(press(KeyCode::Enter)).await;

    let toast = app.toast.as_ref().expect("no toast");
    assert_eq!(toast.kind, ToastKind::Error);
    assert!(toast.msg.contains("future"), "{}", toast.msg);
    assert!(matches!(app.mode, Mode::Form(_)));
    assert_eq!(stored(&app, id).await, before);
}

/// 00:00-00:10 and 00:20-00:30: the older end moved to 00:21 overlaps; the
/// store's refusal as a toast, the form stays open.
#[tokio::test]
async fn an_overlap_is_refused_and_keeps_the_form() {
    let mut app = list_app(2).await;
    app.clock = || at(20, 0);
    app.handle_key(key('e')).await; // list, on the newest
    app.handle_key(key('j')).await; // the older one
    app.handle_key(key('e')).await; // its form
    let id = app.session_list.selected.unwrap();

    app.handle_key(press(KeyCode::Tab)).await; // end
    to_minute_segment(&mut app).await;
    for _ in 0..11 {
        app.handle_key(press(KeyCode::Up)).await; // 00:21
    }
    app.handle_key(press(KeyCode::Enter)).await;

    let toast = app.toast.as_ref().expect("no toast");
    assert_eq!(
        (toast.kind, toast.msg.clone()),
        (ToastKind::Error, SessionError::Overlap.to_string())
    );
    assert!(matches!(app.mode, Mode::Form(_)));
    assert_eq!(stored(&app, id).await.end, Some(at(0, 10)));
}

/// A running session: only its start, moved a minute back; it keeps
/// running.
#[tokio::test]
async fn a_running_session_only_moves_its_start() {
    let mut app = list_app(0).await;
    app.clock = || at(20, 0);
    app.core.start(&[0], at(9, 0)).await.unwrap();
    app.reload().await;
    app.handle_key(key('e')).await; // list
    app.handle_key(key('e')).await; // form
    let id = app.session_list.selected.unwrap();
    let Mode::Form(form) = &app.mode else {
        panic!("no form open");
    };
    assert_eq!(form.fields.len(), 1);

    to_minute_segment(&mut app).await;
    app.handle_key(press(KeyCode::Down)).await; // 08:59
    app.handle_key(press(KeyCode::Enter)).await;

    let s = stored(&app, id).await;
    assert_eq!((s.start, s.end), (at(8, 59), None));
    assert!(app.toast.as_ref().unwrap().msg.ends_with("–now"));
}

// ---------- split (`s`) and cut (`c`) in the list ----------
// `list_app(1)`: "a" 00:00–00:10, so split starts on 00:05 and cut on
// 00:05–00:10 (its 30 minutes capped at the end). "now" is 20:00.

/// `list_app(1)` with a fixed clock, in the list on its only session.
async fn one_session_in_the_list() -> App<'static> {
    let mut app = list_app(1).await;
    app.clock = || at(20, 0);
    app.handle_key(key('e')).await;
    app
}

/// "a" running since 19:00 (now 20:00), in the list on it.
async fn running_in_the_list() -> App<'static> {
    let mut app = list_app(0).await;
    app.clock = || at(20, 0);
    app.core.start(&[0], at(19, 0)).await.unwrap();
    app.reload().await;
    app.handle_key(key('e')).await;
    app
}

/// `14:05`: local, like the toasts.
fn hm(t: Time) -> String {
    t.with_timezone(&chrono::Local).format("%H:%M").to_string()
}

/// The selected session's (start, end).
fn selected_times(app: &App<'_>) -> (Time, Option<Time>) {
    let id = app.session_list.selected.expect("nothing selected");
    let s = app
        .sessions
        .iter()
        .find(|s| s.id == id)
        .expect("not loaded");
    (s.start, s.end)
}

fn toast_msg<'a>(app: &'a App<'_>) -> &'a str {
    &app.toast.as_ref().expect("no toast").msg
}

#[tokio::test]
async fn s_then_enter_splits_and_selects_the_earlier_half() {
    let mut app = one_session_in_the_list().await;

    app.handle_key(key('s')).await;
    app.handle_key(press(KeyCode::Enter)).await;

    assert_eq!(app.mode, Mode::Sessions);
    assert_eq!(app.sessions.len(), 2);
    assert_eq!(selected_times(&app), (at(0, 0), Some(at(0, 5))));
    assert_eq!(toast_msg(&app), format!("split a at {}", hm(at(0, 5))));
}

/// `at` moved onto the start: nothing to split, the form stays to fix it.
#[tokio::test]
async fn a_split_at_the_start_is_refused_and_the_form_stays() {
    let mut app = one_session_in_the_list().await;
    app.handle_key(key('s')).await;

    to_minute_segment(&mut app).await;
    for _ in 0..5 {
        app.handle_key(press(KeyCode::Down)).await; // 00:05 -> 00:00
    }
    app.handle_key(press(KeyCode::Enter)).await;

    assert!(matches!(app.mode, Mode::Form(_)), "{:?}", app.mode);
    assert_eq!(toast_msg(&app), crate::core::SPLIT_AT_EDGE);
    assert_eq!(app.sessions.len(), 1);
}

#[tokio::test]
async fn s_on_a_running_session_says_stop_the_timer() {
    let mut app = running_in_the_list().await;

    app.handle_key(key('s')).await;

    assert_eq!(app.mode, Mode::Sessions); // no form
    let toast = app.toast.as_ref().expect("no toast");
    assert_eq!(
        (toast.kind, toast.msg.as_str()),
        (ToastKind::Error, "stop the timer to split")
    );
}

#[tokio::test]
async fn c_then_enter_cuts_and_selects_what_is_left() {
    let mut app = one_session_in_the_list().await;

    app.handle_key(key('c')).await;
    app.handle_key(press(KeyCode::Enter)).await;

    assert_eq!(app.mode, Mode::Sessions);
    assert_eq!(app.sessions.len(), 1);
    assert_eq!(selected_times(&app), (at(0, 0), Some(at(0, 5))));
    let msg = format!("cut {}–{} from a", hm(at(0, 5)), hm(at(0, 10)));
    assert_eq!(toast_msg(&app), msg);
}

/// 19:00–now (20:00): the cut 19:30–20:00 leaves 19:00–19:30 and a piece
/// from 20:00 that keeps running; the earlier one is selected.
#[tokio::test]
async fn a_cut_on_a_running_session_keeps_the_last_piece_running() {
    let mut app = running_in_the_list().await;

    app.handle_key(key('c')).await;
    app.handle_key(press(KeyCode::Enter)).await;

    assert_eq!(app.mode, Mode::Sessions);
    assert_eq!(app.sessions.len(), 2);
    assert_eq!(selected_times(&app), (at(19, 0), Some(at(19, 30))));
    let running = app.running.as_ref().expect("the timer stopped");
    assert_eq!((running.start, running.end), (at(20, 0), None));
}

/// `from` moved onto the start: 00:00–00:10 is the whole session.
#[tokio::test]
async fn a_cut_over_everything_is_refused_and_the_form_stays() {
    let mut app = one_session_in_the_list().await;
    app.handle_key(key('c')).await;

    to_minute_segment(&mut app).await; // `from`
    for _ in 0..5 {
        app.handle_key(press(KeyCode::Down)).await; // 00:05 -> 00:00
    }
    app.handle_key(press(KeyCode::Enter)).await;

    assert!(matches!(app.mode, Mode::Form(_)), "{:?}", app.mode);
    let toast = app.toast.as_ref().expect("no toast");
    let whole = SessionError::WholeSession.to_string();
    assert_eq!((toast.kind, &toast.msg), (ToastKind::Error, &whole));
    assert_eq!(selected_times(&app), (at(0, 0), Some(at(0, 10)))); // untouched
}

/// Esc: back to the list, same selection; while open, the tree stays
/// dimmed (`in_list`).
#[tokio::test]
async fn esc_in_add_split_and_cut_goes_back_to_the_list() {
    for open in [key('a'), key('s'), key('c')] {
        let mut app = one_session_in_the_list().await;
        let selected = app.session_list.selected;

        app.handle_key(open).await;
        assert!(matches!(app.mode, Mode::Form(_)), "{open:?} opened no form");
        assert!(app.in_list(), "{open:?}");
        app.handle_key(press(KeyCode::Esc)).await;

        assert_eq!(app.mode, Mode::Sessions, "{open:?}");
        assert_eq!(app.session_list.selected, selected, "{open:?}");
        assert_eq!(app.sessions.len(), 1, "{open:?}");
    }
}

#[tokio::test]
async fn s_and_c_in_the_empty_list_say_no_session_selected() {
    for open in [key('s'), key('c')] {
        let mut app = list_app(0).await;
        app.handle_key(key('e')).await; // the empty list

        app.handle_key(open).await;

        assert_eq!(app.mode, Mode::Sessions, "{open:?}");
        assert_eq!(toast_msg(&app), "no session selected", "{open:?}");
    }
}

// ---------- add (`a`) in the list ----------
// The add form starts on the last hour: now 20:00 -> 19:00–20:00.

#[tokio::test]
async fn a_then_enter_adds_and_selects_the_new_session() {
    let mut app = one_session_in_the_list().await;

    app.handle_key(key('a')).await;
    app.handle_key(press(KeyCode::Enter)).await;

    assert_eq!(app.mode, Mode::Sessions);
    assert_eq!(app.sessions.len(), 2);
    assert_eq!(selected_times(&app), (at(19, 0), Some(at(20, 0))));
    let msg = format!("added a: {}–{} (1h)", hm(at(19, 0)), hm(at(20, 0)));
    assert_eq!(toast_msg(&app), msg);
}

#[tokio::test]
async fn a_in_the_empty_list_adds_the_first_session() {
    let mut app = list_app(0).await;
    app.clock = || at(20, 0);
    app.handle_key(key('e')).await; // the empty list

    app.handle_key(key('a')).await;
    app.handle_key(press(KeyCode::Enter)).await;

    assert_eq!(app.mode, Mode::Sessions);
    assert_eq!(app.sessions.len(), 1);
    assert_eq!(selected_times(&app), (at(19, 0), Some(at(20, 0))));
}

/// A container's list holds the sessions of every task below it: which
/// task would a new one be on? Also from the root.
#[tokio::test]
async fn a_on_a_containers_list_says_pick_a_task() {
    for cursor in [&[1][..], &[][..]] {
        let mut app = test_app(tree(), state_at(cursor)); // "ws" / the root
        app.clock = || at(20, 0);
        app.reload().await;
        app.details_tab = DetailsTab::Sessions;
        app.handle_key(key('e')).await; // its (empty) list

        app.handle_key(key('a')).await;

        assert_eq!(app.mode, Mode::Sessions, "{cursor:?}"); // no form
        let toast = app.toast.as_ref().expect("no toast");
        let expected = (ToastKind::Error, "pick a task to add a session");
        assert_eq!((toast.kind, toast.msg.as_str()), expected, "{cursor:?}");
    }
}

/// Now 00:30: the last hour (23:30–00:30) runs over "a" 00:00–00:10.
#[tokio::test]
async fn an_overlapping_add_is_refused_and_the_form_stays() {
    let mut app = list_app(1).await;
    app.clock = || at(0, 30);
    app.handle_key(key('e')).await;

    app.handle_key(key('a')).await;
    app.handle_key(press(KeyCode::Enter)).await;

    assert!(matches!(app.mode, Mode::Form(_)), "{:?}", app.mode);
    let toast = app.toast.as_ref().expect("no toast");
    let overlap = SessionError::Overlap.to_string();
    assert_eq!((toast.kind, &toast.msg), (ToastKind::Error, &overlap));
    assert_eq!(app.sessions.len(), 1);
}

/// The end one minute after now (20:01): sessions record what happened.
#[tokio::test]
async fn an_add_in_the_future_is_refused_and_the_form_stays() {
    let mut app = one_session_in_the_list().await;
    app.handle_key(key('a')).await;

    app.handle_key(press(KeyCode::Tab)).await; // `end`
    to_minute_segment(&mut app).await;
    app.handle_key(press(KeyCode::Up)).await; // 20:01
    app.handle_key(press(KeyCode::Enter)).await;

    assert!(matches!(app.mode, Mode::Form(_)), "{:?}", app.mode);
    let toast = app.toast.as_ref().expect("no toast");
    assert_eq!(toast.kind, ToastKind::Error);
    assert!(toast.msg.contains("future"), "got: {}", toast.msg);
    assert_eq!(app.sessions.len(), 1);
}

// ---------- open (`o`) and back from the script ----------

/// `tree()` with `run_dir` in a temp folder holding an executable `editor`,
/// and `open_with = editor` on the root (inherited by everything).
fn tree_with_editor() -> (tempfile::TempDir, Tree) {
    let run = tempfile::tempdir().unwrap();
    let script = run.path().join("editor");
    fs::write(&script, "#!/bin/sh\n").unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
    let mut t = tree();
    let root = t.get_mut(&[]).and_then(Node::as_container_mut).unwrap();
    root.root_settings.run_dir = Some(run.path().to_path_buf());
    root.settings.open_with = Some("editor".parse().unwrap());
    (run, t)
}

/// The error toast's text; fails without one.
fn error_toast(app: &App) -> String {
    let toast = app.toast.as_ref().expect("no toast");
    assert_eq!(toast.kind, ToastKind::Error);
    toast.msg.clone()
}

#[tokio::test]
async fn o_on_a_task_asks_the_loop_to_run_its_open_with() {
    let (run, t) = tree_with_editor();
    let b = t.get(&[1, 0]).unwrap().id();
    let mut app = test_app(t, state_at(&[1, 0]));

    let Flow::Run(request) = app.handle_key(key('o')).await else {
        panic!("no run, toast: {:?}", app.toast);
    };

    assert_eq!(request.name.as_str(), "editor");
    assert_eq!(request.script, run.path().join("editor"));
    assert_eq!(request.ctx.event, Event::Open);
    assert_eq!(request.ctx.node.id, b);
    assert_eq!(request.ctx.task.as_ref().map(|t| t.id), Some(b)); // a task times itself
    assert!(app.toast.is_none());
}

/// Problems are toasts; the TUI stays (no `Flow::Run`).
#[tokio::test]
async fn o_without_open_with_says_what_to_set() {
    let mut app = test_app(tree(), state_at(&[1, 0]));

    assert_eq!(app.handle_key(key('o')).await, Flow::Continue);
    assert_eq!(error_toast(&app), "no run config for b (set open_with)");
}

#[tokio::test]
async fn o_with_an_unknown_script_lists_the_ones_there_are() {
    let (_run, mut t) = tree_with_editor();
    let root = t.get_mut(&[]).and_then(Node::as_container_mut).unwrap();
    root.settings.open_with = Some("nope".parse().unwrap());
    let mut app = test_app(t, state_at(&[1, 0]));

    assert_eq!(app.handle_key(key('o')).await, Flow::Continue);
    let msg = error_toast(&app);
    assert!(msg.starts_with("no run config \"nope\""), "{msg}");
    assert!(msg.ends_with("(have: editor)"), "{msg}");
}

#[tokio::test]
async fn o_on_a_script_without_x_says_chmod() {
    let (run, t) = tree_with_editor();
    let script = run.path().join("editor");
    fs::set_permissions(&script, fs::Permissions::from_mode(0o644)).unwrap();
    let mut app = test_app(t, state_at(&[1, 0]));

    assert_eq!(app.handle_key(key('o')).await, Flow::Continue);
    assert!(error_toast(&app).contains("chmod +x"));
}

/// `ws` has only `b`: no picker, the time goes to `b`.
#[tokio::test]
async fn o_on_a_container_with_one_open_task_takes_it() {
    let (_run, t) = tree_with_editor();
    let (ws, b) = (t.get(&[1]).unwrap().id(), t.get(&[1, 0]).unwrap().id());
    let mut app = test_app(t, state_at(&[1]));

    let Flow::Run(request) = app.handle_key(key('o')).await else {
        panic!("no run: {:?} {:?}", app.mode, app.toast);
    };

    assert_eq!(request.ctx.node.id, ws, "the container is opened");
    assert_eq!(request.ctx.task.as_ref().map(|t| t.id), Some(b));
}

#[tokio::test]
async fn o_on_a_container_without_open_tasks_says_so() {
    let (_run, mut t) = tree_with_editor();
    set_done(&mut t, &[1, 0]);
    let mut app = test_app(t, state_at(&[1]));

    assert_eq!(app.handle_key(key('o')).await, Flow::Continue);
    assert_eq!(error_toast(&app), "no open task in ws");
    assert_eq!(app.mode, Mode::Normal);
}

#[tokio::test]
async fn o_in_the_sessions_list_does_nothing() {
    let (_run, t) = tree_with_editor();
    let mut app = test_app(t, state_at(&[1, 0]));
    app.details_tab = DetailsTab::Sessions;
    app.handle_key(key('e')).await; // into the list
    assert_eq!(app.mode, Mode::Sessions);

    assert_eq!(app.handle_key(key('o')).await, Flow::Continue);
    assert!(app.toast.is_none());
}

/// Exit code `code`, as a finished child reports it.
fn exited(code: i32) -> ExitStatus {
    ExitStatus::from_raw(code << 8) // a wait status: the code is the high byte
}

#[tokio::test]
async fn after_a_script_that_failed_a_toast_says_so() {
    let mut app = test_app(tree(), state_at(&[1, 0]));

    app.after_run(&"editor".parse().unwrap(), Ok(exited(1)))
        .await;

    assert_eq!(error_toast(&app), "editor exited with 1");
}

#[tokio::test]
async fn after_a_script_that_succeeded_nothing_is_said() {
    let mut app = test_app(tree(), state_at(&[1, 0]));

    app.after_run(&"editor".parse().unwrap(), Ok(exited(0)))
        .await;

    assert!(app.toast.is_none());
}

#[tokio::test]
async fn after_a_script_that_did_not_start_the_toast_has_the_hint() {
    let mut app = test_app(tree(), state_at(&[1, 0]));
    let error = RunError::Launch {
        path: "/run/editor".into(),
        error: std::io::ErrorKind::NotFound.into(),
    };

    app.after_run(&"editor".parse().unwrap(), Err(error)).await;

    assert!(error_toast(&app).ends_with("(check its #! line)"));
}

/// The script may have started a timer (`udo track`): `after_run` reads
/// it, so the status line shows it right away.
#[tokio::test]
async fn after_a_script_its_timer_shows() {
    let mut app = test_app(tree(), state_at(&[1, 0]));
    assert!(app.running.is_none());
    let (source, owner) = ("editor".parse().unwrap(), "editor:1".parse().unwrap());
    app.core
        .track_start(&[1, 0], source, owner, at(14, 0))
        .await
        .unwrap();

    app.after_run(&"editor".parse().unwrap(), Ok(exited(0)))
        .await;

    assert_eq!(
        app.running.as_ref().map(|s| s.task.name.as_str()),
        Some("b")
    );
}

// ---------- picker keys (`Mode::Pick`) ----------

/// The app on `a` with a script picker over `x`, `y`, `z` open, cursor on
/// `x`. Put in place directly: these tests are about the keys, not about
/// how `o` / `O` open it.
fn picker_app() -> App<'static> {
    let mut app = test_app(tree(), state_at(&[0]));
    let items = ["x", "y", "z"]
        .map(|n| PickItem {
            label: n.into(),
            note: None,
            value: PickValue::Script(n.parse().unwrap()),
        })
        .to_vec();
    app.mode = Mode::Pick(Box::new(Picker {
        title: "open a with".into(),
        items,
        cursor: 0,
        action: PickAction::Script { path: vec![0] },
    }));
    app
}

fn picker_cursor(app: &App) -> usize {
    match &app.mode {
        Mode::Pick(p) => p.cursor,
        other => panic!("no picker: {other:?}"),
    }
}

#[tokio::test]
async fn j_and_k_move_in_the_picker_and_stop_at_the_ends() {
    let mut app = picker_app();

    app.handle_key(key('k')).await;
    assert_eq!(picker_cursor(&app), 0, "stays on the first");
    app.handle_key(key('j')).await;
    assert_eq!(picker_cursor(&app), 1);
    app.handle_key(press(KeyCode::Down)).await;
    app.handle_key(key('j')).await;
    assert_eq!(picker_cursor(&app), 2, "stays on the last");
    app.handle_key(press(KeyCode::Up)).await;
    assert_eq!(picker_cursor(&app), 1);
    assert_eq!(app.tree_state.cursor, vec![0], "the tree did not move");
}

#[tokio::test]
async fn esc_closes_the_picker_and_runs_nothing() {
    let mut app = picker_app();
    app.handle_key(key('j')).await;

    assert_eq!(app.handle_key(press(KeyCode::Esc)).await, Flow::Continue);

    assert_eq!(app.mode, Mode::Normal);
    assert_eq!(app.tree_state.cursor, vec![0]);
    assert!(app.toast.is_none());
}

/// Help shows the picker's keys; any key brings the picker back as it was.
#[tokio::test]
async fn help_over_the_picker_shows_its_keys_and_returns_to_it() {
    let mut app = picker_app();
    app.handle_key(key('j')).await;
    let picker = app.mode.clone();

    app.handle_key(key('?')).await;
    let Mode::Help(under) = &app.mode else {
        panic!("no help: {:?}", app.mode);
    };
    let keys = under.keymap().expect("a keymap to show");
    assert!(keys::bindings_in(keys).any(|b| b.action == Action::Pick));
    assert_eq!(keys.len(), PICKER_KEYMAP.len());

    app.handle_key(key('j')).await; // only closes the help
    assert_eq!(app.mode, picker);
}

/// Tree keys mean nothing in the picker: no change, and `q` does not quit.
#[tokio::test]
async fn tree_keys_do_nothing_in_the_picker() {
    let mut app = picker_app();
    let picker = app.mode.clone();

    // `d` / `t` would open a prompt / form: the mode check covers them
    for c in ['x', 'd', 's', 't', 'o', 'q', 'l', 'h'] {
        assert_eq!(app.handle_key(key(c)).await, Flow::Continue, "{c}");
    }

    assert_eq!(app.mode, picker);
    let a = app.core.tree().get(&[0]).and_then(Node::as_task).unwrap();
    assert!(a.done_at.is_none(), "x did not mark it done");
    assert_eq!(app.tree_state.cursor, vec![0]);
    assert!(app.running.is_none(), "no timer started");
}

/// Opened from the tree: its cursor stays lit, it shows what is opened.
#[test]
fn the_tree_cursor_stays_lit_while_picking() {
    let mut app = picker_app();
    assert!(!app.in_list());

    app.mode = Mode::Help(Box::new(app.mode.clone()));
    assert!(!app.in_list(), "under the help too");
}

// ---------- the pickers of `o` / `O` ----------

fn set_done(t: &mut Tree, path: &[usize]) {
    let NodeBody::Task(task) = &mut t.get_mut(path).unwrap().body else {
        panic!("{path:?} is no task");
    };
    task.done_at = Some(at(8, 0));
}

fn set_due(t: &mut Tree, path: &[usize], due: Time) {
    let NodeBody::Task(task) = &mut t.get_mut(path).unwrap().body else {
        panic!("{path:?} is no task");
    };
    task.due_date = due;
}

/// An executable `name` in the run folder `run`.
fn add_script(run: &Path, name: &str) {
    let script = run.join(name);
    fs::write(&script, "#!/bin/sh\n").unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
}

/// root: [a, ws: [b, wk: [c], d]], scripts `editor` and `shell`,
/// `open_with = shell` (the second name: a preselection must move the
/// cursor). Due: b 12:00, c 9:00, d 9:00 (c and d tie, c comes first).
fn picker_tree() -> (tempfile::TempDir, Tree) {
    let run = tempfile::tempdir().unwrap();
    add_script(run.path(), "editor");
    add_script(run.path(), "shell");
    let mut t = tree_with(vec![
        task("a"),
        container(
            "ws",
            vec![task("b"), container("wk", vec![task("c")]), task("d")],
        ),
    ]);
    let root = t.get_mut(&[]).and_then(Node::as_container_mut).unwrap();
    root.root_settings.run_dir = Some(run.path().to_path_buf());
    root.settings.open_with = Some("shell".parse().unwrap());
    set_due(&mut t, &[1, 0], at(12, 0));
    set_due(&mut t, &[1, 1, 0], at(9, 0));
    set_due(&mut t, &[1, 2], at(9, 0));
    (run, t)
}

/// The open picker; fails without one.
fn picker<'a>(app: &'a App) -> &'a Picker {
    match &app.mode {
        Mode::Pick(p) => p,
        other => panic!("no picker: {other:?}, toast {:?}", app.toast),
    }
}

fn labels(p: &Picker) -> Vec<&str> {
    p.items.iter().map(|i| i.label.as_str()).collect()
}

#[tokio::test]
async fn o_on_a_container_with_more_open_tasks_opens_the_task_picker() {
    let (_run, t) = picker_tree();
    let mut app = test_app(t, state_at(&[1]));

    assert_eq!(app.handle_key(key('o')).await, Flow::Continue);

    let p = picker(&app);
    assert_eq!(p.title, "open ws for");
    assert_eq!(labels(p), ["b", "wk / c", "d"], "paths below ws");
    assert_eq!(p.cursor, 1, "c: due first, before d in the tree");
    let script = "shell".parse().unwrap();
    assert_eq!(
        p.action,
        PickAction::Task {
            container: vec![1],
            script
        }
    );
}

#[tokio::test]
async fn the_task_picker_skips_done_tasks() {
    let (_run, mut t) = picker_tree();
    set_done(&mut t, &[1, 1, 0]);
    let mut app = test_app(t, state_at(&[1]));

    app.handle_key(key('o')).await;

    let p = picker(&app);
    assert_eq!(labels(p), ["b", "d"]);
    assert_eq!(p.cursor, 1, "d is due first now");
}

/// Enter: the container is opened, the time goes to the picked task.
#[tokio::test]
async fn enter_in_the_task_picker_runs_for_the_picked_task() {
    let (run, t) = picker_tree();
    let (ws, d) = (t.get(&[1]).unwrap().id(), t.get(&[1, 2]).unwrap().id());
    let mut app = test_app(t, state_at(&[1]));
    app.handle_key(key('o')).await;
    app.handle_key(key('j')).await; // c -> d

    let Flow::Run(request) = app.handle_key(press(KeyCode::Enter)).await else {
        panic!("no run: {:?} {:?}", app.mode, app.toast);
    };

    assert_eq!(request.name.as_str(), "shell");
    assert_eq!(request.script, run.path().join("shell"));
    assert_eq!(request.ctx.node.id, ws);
    assert_eq!(request.ctx.task.as_ref().map(|t| t.id), Some(d));
    assert_eq!(app.mode, Mode::Normal, "the picker is closed");
}

#[tokio::test]
async fn shift_o_opens_the_script_picker_on_the_default() {
    let (_run, t) = picker_tree();
    let mut app = test_app(t, state_at(&[0]));

    assert_eq!(app.handle_key(key('O')).await, Flow::Continue);

    let p = picker(&app);
    assert_eq!(p.title, "open a with");
    assert_eq!(labels(p), ["editor", "shell"], "sorted");
    let notes: Vec<_> = p.items.iter().map(|i| i.note).collect();
    assert_eq!(notes, [None, Some("(default)")]);
    assert_eq!(p.cursor, 1, "on open_with");
    assert_eq!(p.action, PickAction::Script { path: vec![0] });
}

#[tokio::test]
async fn shift_o_without_open_with_starts_on_the_first_unmarked() {
    let (_run, mut t) = picker_tree();
    let root = t.get_mut(&[]).and_then(Node::as_container_mut).unwrap();
    root.settings.open_with = None;
    let mut app = test_app(t, state_at(&[0]));

    app.handle_key(key('O')).await;

    let p = picker(&app);
    assert_eq!(p.cursor, 0);
    assert!(p.items.iter().all(|i| i.note.is_none()));
}

/// `O` -> `editor` (not the default) on a task: runs that one.
#[tokio::test]
async fn shift_o_then_enter_on_a_task_runs_the_picked_script() {
    let (run, t) = picker_tree();
    let a = t.get(&[0]).unwrap().id();
    let mut app = test_app(t, state_at(&[0]));
    app.handle_key(key('O')).await;
    app.handle_key(key('k')).await; // shell -> editor

    let Flow::Run(request) = app.handle_key(press(KeyCode::Enter)).await else {
        panic!("no run: {:?} {:?}", app.mode, app.toast);
    };

    assert_eq!(request.name.as_str(), "editor");
    assert_eq!(request.script, run.path().join("editor"));
    assert_eq!(request.ctx.task.as_ref().map(|t| t.id), Some(a));
}

/// `O` on a container: the script first, then the task picker with it.
#[tokio::test]
async fn shift_o_then_enter_on_a_container_opens_the_task_picker() {
    let (_run, t) = picker_tree();
    let mut app = test_app(t, state_at(&[1]));
    app.handle_key(key('O')).await;
    app.handle_key(key('k')).await; // shell -> editor

    assert_eq!(app.handle_key(press(KeyCode::Enter)).await, Flow::Continue);

    let script = "editor".parse().unwrap();
    assert_eq!(
        picker(&app).action,
        PickAction::Task {
            container: vec![1],
            script
        }
    );
}

/// Checked before the script picker: the pick would lead nowhere.
#[tokio::test]
async fn shift_o_on_a_container_without_open_tasks_says_so_first() {
    let (_run, mut t) = tree_with_editor();
    set_done(&mut t, &[1, 0]);
    let mut app = test_app(t, state_at(&[1]));

    app.handle_key(key('O')).await;

    assert_eq!(error_toast(&app), "no open task in ws");
    assert_eq!(app.mode, Mode::Normal);
}

#[tokio::test]
async fn shift_o_with_an_empty_library_says_where_it_looked() {
    let run = tempfile::tempdir().unwrap();
    let mut t = tree();
    let root = t.get_mut(&[]).and_then(Node::as_container_mut).unwrap();
    root.root_settings.run_dir = Some(run.path().to_path_buf());
    let mut app = test_app(t, state_at(&[0]));

    app.handle_key(key('O')).await;

    let expected = format!("no run configs in {}", run.path().display());
    assert_eq!(error_toast(&app), expected);
    assert_eq!(app.mode, Mode::Normal);
}

/// The library is read again on Enter: a script gone meanwhile is a
/// toast, not a run.
#[tokio::test]
async fn a_picked_script_that_is_gone_is_a_toast() {
    let (run, t) = picker_tree();
    let mut app = test_app(t, state_at(&[0]));
    app.handle_key(key('O')).await;
    fs::remove_file(run.path().join("shell")).unwrap();

    assert_eq!(app.handle_key(press(KeyCode::Enter)).await, Flow::Continue);

    assert!(error_toast(&app).starts_with("no run config \"shell\""));
    assert_eq!(app.mode, Mode::Normal);
}

// ---------- `on_create` after a create form (the `setup` row) ----------

/// An empty root on disk in `tmp` with a script `setup` in its run folder
/// (`<root>/run`); `on_create = setup` on the root if `on_create`.
async fn setup_app(tmp: &Path, on_create: bool) -> App<'static> {
    run_script(tmp, "setup", "");
    let mut t = Tree::load_from(tmp).await.unwrap();
    if on_create {
        let root = t.get_mut(&[]).and_then(Node::as_container_mut).unwrap();
        root.settings.on_create = Some("setup".parse().unwrap());
    }
    test_app(t, TreeState::default())
}

/// `form_key` (`t` / `c`), `name`, Enter: what the save returned.
async fn create(app: &mut App<'_>, form_key: char, name: &str) -> Flow {
    app.handle_key(key(form_key)).await;
    type_into(app, name).await;
    app.handle_key(press(KeyCode::Enter)).await
}

#[tokio::test]
async fn the_create_forms_get_the_setup_row_with_on_create() {
    let tmp = tempfile::tempdir().unwrap();
    let mut app = setup_app(tmp.path(), true).await;

    for form_key in ['t', 'c'] {
        app.handle_key(key(form_key)).await;

        let form = open_form(&app);
        assert_eq!(
            form.fields.last().map(|f| f.id),
            Some(FieldId::Setup),
            "{form_key}"
        );
        assert!(form.runs_setup(), "{form_key}: run is the default");
        app.handle_key(press(KeyCode::Esc)).await;
    }
}

/// Without `on_create` the forms stay as they were, and saving runs
/// nothing.
#[tokio::test]
async fn without_on_create_no_row_and_nothing_runs() {
    let tmp = tempfile::tempdir().unwrap();
    let mut app = setup_app(tmp.path(), false).await;

    app.handle_key(key('t')).await;
    assert!(
        open_form(&app)
            .fields
            .iter()
            .all(|f| f.id != FieldId::Setup)
    );
    app.handle_key(press(KeyCode::Esc)).await;

    assert_eq!(create(&mut app, 't', "exam").await, Flow::Continue);
    assert_eq!(app.toast.as_ref().unwrap().kind, ToastKind::Info);
}

/// Saved, then `Flow::Run` for the new task: `create`, the task timed.
#[tokio::test]
async fn saving_a_task_form_runs_on_create_for_the_new_task() {
    let tmp = tempfile::tempdir().unwrap();
    let mut app = setup_app(tmp.path(), true).await;

    let Flow::Run(request) = create(&mut app, 't', "exam").await else {
        panic!("no run: {:?}", app.toast);
    };

    let exam = app.core.tree().get(&[0]).unwrap();
    assert_eq!(exam.name(), "exam", "saved before it runs");
    assert_eq!(request.name.as_str(), "setup");
    assert_eq!(request.script, tmp.path().join("run").join("setup"));
    assert_eq!(request.ctx.event, Event::Create);
    assert_eq!(request.ctx.node.id, exam.id());
    assert_eq!(request.ctx.task.as_ref().map(|t| t.id), Some(exam.id()));
    assert_eq!(app.mode, Mode::Normal, "the form is closed");
    assert_eq!(app.tree_state.cursor, vec![0], "on the new node");
}

/// A container has no task: `UDO_TASK_*` stay unset.
#[tokio::test]
async fn saving_a_container_form_runs_on_create_without_a_task() {
    let tmp = tempfile::tempdir().unwrap();
    let mut app = setup_app(tmp.path(), true).await;

    let Flow::Run(request) = create(&mut app, 'c', "uni").await else {
        panic!("no run: {:?}", app.toast);
    };

    assert_eq!(request.ctx.event, Event::Create);
    assert_eq!(request.ctx.node.name, "uni");
    assert_eq!(request.ctx.task, None);
}

/// Shift+Tab to the last row, → to `skip`: saved, nothing runs.
#[tokio::test]
async fn skip_saves_without_running() {
    let tmp = tempfile::tempdir().unwrap();
    let mut app = setup_app(tmp.path(), true).await;
    app.handle_key(key('t')).await;
    type_into(&mut app, "exam").await;

    app.handle_key(press(KeyCode::BackTab)).await; // wraps to the setup row
    app.handle_key(press(KeyCode::Right)).await;
    assert!(!open_form(&app).runs_setup());

    assert_eq!(app.handle_key(press(KeyCode::Enter)).await, Flow::Continue);
    assert_eq!(app.core.tree().get(&[0]).map(|n| n.name()), Some("exam"));
    assert_eq!(app.mode, Mode::Normal);
}

/// A failed save runs nothing: the form stays open with the error.
#[tokio::test]
async fn a_failed_save_runs_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let mut app = setup_app(tmp.path(), true).await;
    create(&mut app, 't', "exam").await;

    let flow = create(&mut app, 't', "exam").await; // the name exists

    assert_eq!(flow, Flow::Continue);
    assert!(matches!(app.mode, Mode::Form(_)), "the form stays open");
    assert_eq!(app.toast.as_ref().unwrap().kind, ToastKind::Error);
}

/// The script is gone when saving: the node stays, the toast says both.
#[tokio::test]
async fn a_missing_script_is_a_toast_and_the_node_stays() {
    let tmp = tempfile::tempdir().unwrap();
    let mut app = setup_app(tmp.path(), true).await;
    app.handle_key(key('t')).await;
    type_into(&mut app, "exam").await;
    fs::remove_file(tmp.path().join("run").join("setup")).unwrap();

    assert_eq!(app.handle_key(press(KeyCode::Enter)).await, Flow::Continue);

    assert_eq!(app.core.tree().get(&[0]).map(|n| n.name()), Some("exam"));
    let msg = error_toast(&app);
    assert!(msg.starts_with("no run config \"setup\""), "{msg}");
    assert!(msg.ends_with("(exam was added)"), "{msg}");
}

/// The row decides, as shown when the form opened: `on_create` switched
/// off meanwhile runs nothing.
#[tokio::test]
async fn on_create_switched_off_while_the_form_is_open_runs_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let mut app = setup_app(tmp.path(), true).await;
    app.handle_key(key('t')).await;
    type_into(&mut app, "exam").await;
    let settings = crate::model::settings::ContainerSettings {
        on_create: Some("none".parse().unwrap()),
        ..Default::default()
    };
    app.core
        .set_settings(&[], settings, Some(Default::default()))
        .await
        .unwrap();

    assert_eq!(app.handle_key(press(KeyCode::Enter)).await, Flow::Continue);
    assert_eq!(app.core.tree().get(&[0]).map(|n| n.name()), Some("exam"));
}

/// Editing keeps working as before: no row, no run.
#[tokio::test]
async fn the_edit_form_has_no_setup_row() {
    let tmp = tempfile::tempdir().unwrap();
    let mut app = setup_app(tmp.path(), true).await;
    let Flow::Run(_) = create(&mut app, 't', "exam").await else {
        panic!("no run");
    };

    app.handle_key(key('e')).await;
    assert!(
        open_form(&app)
            .fields
            .iter()
            .all(|f| f.id != FieldId::Setup)
    );
    assert_eq!(app.handle_key(press(KeyCode::Enter)).await, Flow::Continue);
}
