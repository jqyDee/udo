use std::path::Path;

use crossterm::event::{KeyCode, KeyEventState, KeyModifiers};

use super::*;
use crate::{
    model::{container::ContainerKind, node::Node, settings::TaskFolderSetting},
    test_util::{container, container_at, press, state_at, task, tree_with},
    tui::{
        form::{FieldId, FolderMode, FormAction, TextInput},
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
    let mut t = tree();
    let mut app = App::new(&mut t, state_at(&[0]));
    assert_eq!(app.handle_key(key('q')).await, Flow::Quit);
}

#[tokio::test]
async fn help_opens_and_any_key_closes_it_without_acting() {
    let mut t = tree();
    let mut app = App::new(&mut t, state_at(&[0]));

    app.handle_key(key('?')).await;
    assert_eq!(app.mode, Mode::Help);

    // `j` only closes the help, the cursor must not move
    assert_eq!(app.handle_key(key('j')).await, Flow::Continue);
    assert_eq!(app.mode, Mode::Normal);
    assert_eq!(app.tree_state.cursor, vec![0]);
}

#[tokio::test]
async fn q_in_help_closes_help_instead_of_quitting() {
    let mut t = tree();
    let mut app = App::new(&mut t, state_at(&[0]));
    app.handle_key(key('?')).await;
    assert_eq!(app.handle_key(key('q')).await, Flow::Continue);
    assert_eq!(app.mode, Mode::Normal);
}

#[tokio::test]
async fn key_release_is_ignored() {
    let mut t = tree();
    let mut app = App::new(&mut t, state_at(&[0]));
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
    let mut t = tree();
    let mut app = App::new(&mut t, state_at(&[0]));
    assert_eq!(app.handle_key(key('#')).await, Flow::Continue);
    assert_eq!(app.tree_state.cursor, vec![0]);
    assert!(app.toast.is_none());
}

#[test]
fn new_selects_first_row() {
    let mut t = tree();
    let app = App::new(&mut t, TreeState::default());
    assert_eq!(app.tree_state.cursor, vec![0]);
}

// ---------- actions ----------

#[tokio::test]
async fn navigation_moves_cursor_without_toast() {
    let mut t = tree();
    let mut app = App::new(&mut t, state_at(&[0]));
    app.handle_key(key('j')).await;
    assert_eq!(app.tree_state.cursor, vec![1]);
    assert!(app.toast.is_none());
}

#[tokio::test]
async fn folding_hides_rows_without_touching_the_tree() {
    let mut t = tree();
    let mut app = App::new(&mut t, state_at(&[1])); // "ws"
    app.handle_key(key(' ')).await;
    assert_eq!(app.tree_state.rows(app.tree).len(), 2); // "b" hidden
    app.handle_key(key('j')).await;
    assert_eq!(app.tree_state.cursor, vec![1]); // nothing below "ws"
}

#[tokio::test]
async fn set_status_on_container_shows_error_toast() {
    let mut t = tree();
    let mut app = App::new(&mut t, state_at(&[1])); // "ws": fails before any save

    assert_eq!(app.handle_key(key('x')).await, Flow::Continue);

    let toast = app.toast.as_ref().expect("no toast");
    assert_eq!(toast.kind, ToastKind::Error);
    assert!(toast.msg.contains("only tasks"), "got: {}", toast.msg);
}

#[tokio::test]
async fn set_status_on_task_updates_and_shows_info_toast() {
    let tmp = tempfile::tempdir().unwrap();
    let mut t = Tree::load_from(tmp.path()).await.unwrap(); // real root: saving works
    let path = t.create(&[], task("sheet")).await.unwrap();
    let mut app = App::new(&mut t, state_at(&path));

    app.handle_key(key('x')).await;

    let toast = app.toast.as_ref().expect("no toast");
    assert_eq!(toast.kind, ToastKind::Info);
    assert_eq!(toast.msg, "sheet -> done");
    let sheet = app.tree.get(&path).and_then(Node::as_task);
    assert_eq!(sheet.expect("expected a task").status, TaskStatus::Finished);
}

// ---------- delete + confirm ----------
// The in-memory `tree()` is never saved: only answers that don't delete
// (n, esc, ignored keys) use it. `y` saves, so it gets a real tempdir tree.

#[tokio::test]
async fn d_opens_confirm_for_selected_node() {
    let mut t = tree();
    let mut app = App::new(&mut t, state_at(&[0]));

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
        let mut t = tree();
        let mut app = App::new(&mut t, state_at(&[0]));
        app.handle_key(key('d')).await;

        app.handle_key(cancel).await;

        assert_eq!(app.mode, Mode::Normal, "{cancel:?} did not close");
        assert_eq!(app.tree.get(&[0]).unwrap().name(), "a");
        assert!(app.toast.is_none());
    }
}

#[tokio::test]
async fn other_keys_are_ignored_while_confirming() {
    let mut t = tree();
    let mut app = App::new(&mut t, state_at(&[0]));
    app.handle_key(key('d')).await;

    // neither moves the cursor behind the prompt nor quits
    assert_eq!(app.handle_key(key('j')).await, Flow::Continue);
    assert_eq!(app.handle_key(key('q')).await, Flow::Continue);

    assert!(matches!(app.mode, Mode::Confirm(_)));
    assert_eq!(app.tree_state.cursor, vec![0]);
}

#[tokio::test]
async fn d_without_selection_shows_error() {
    let mut t = tree_with(vec![]); // empty tree: cursor stays on the root
    let mut app = App::new(&mut t, TreeState::default());

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
    let mut app = App::new(&mut t, state_at(&path));

    app.handle_key(key('d')).await;
    app.handle_key(key('y')).await;

    assert_eq!(app.mode, Mode::Normal);
    assert!(app.tree.get(&path).is_none());
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

/// Fake Trash: deletes for real (inside the tempdir only).
fn fake_trash(p: &Path) -> Result<(), String> {
    std::fs::remove_dir_all(p).map_err(|e| e.to_string())
}

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
    let mut t = lab_tree(tmp.path()).await;
    let mut app = App::new(&mut t, state_at(&[0]));

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
    let mut t = tree();
    let mut app = App::new(&mut t, state_at(&[0]));

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
    let mut app = App::new(&mut t, state_at(&[0]));

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
    let mut t = lab_tree(tmp.path()).await;
    let lab = tmp.path().join("lab");
    let mut app = App::new(&mut t, state_at(&[0]));
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
        assert_eq!(app.tree.get(&[0]).unwrap().name(), "lab");
        app.handle_key(press(KeyCode::Esc)).await; // next round from Normal
    }
}

#[tokio::test]
async fn exact_path_deletes_node_and_folder() {
    let tmp = tempfile::tempdir().unwrap();
    let mut t = lab_tree(tmp.path()).await;
    let lab = tmp.path().join("lab");
    let mut app = App::new(&mut t, state_at(&[0]));
    app.trash = fake_trash;

    app.handle_key(key('d')).await;
    app.handle_key(shift_d()).await;
    type_into(&mut app, &lab.display().to_string()).await;
    app.handle_key(press(KeyCode::Enter)).await;

    assert_eq!(app.mode, Mode::Normal);
    assert!(!lab.exists());
    assert!(app.tree.get(&[0]).is_none());
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
    let mut t = lab_tree(tmp.path()).await;
    let mut app = App::new(&mut t, state_at(&[0]));
    app.trash = fake_trash;

    app.handle_key(key('d')).await;
    app.handle_key(shift_d()).await;
    type_into(&mut app, "x").await;
    app.handle_key(press(KeyCode::Esc)).await;

    assert_eq!(app.mode, Mode::Normal); // closed, not back to Ask
    assert!(tmp.path().join("lab").exists());
    assert_eq!(app.tree.get(&[0]).unwrap().name(), "lab");
    assert!(app.toast.is_none());
}

// ---------- toast lifetime ----------

#[test]
fn toast_deadline_and_expiry() {
    let mut t = tree();
    let mut app = App::new(&mut t, state_at(&[0]));
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
        let mut app = App::new(&mut t, state_at(&[1])); // "ws": inherits from the root

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
    let mut app = App::new(&mut t, state_at(&[1])); // "ws": inherits from the root

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
    let mut t = Tree::load_from(tmp.path()).await.unwrap(); // empty root
    let mut app = App::new(&mut t, TreeState::default());

    app.handle_key(key('t')).await;
    assert!(matches!(app.mode, Mode::Form(_)));
    for k in type_str("exam") {
        app.handle_key(k).await;
    }
    app.handle_key(press(KeyCode::Enter)).await;

    assert_eq!(app.mode, Mode::Normal);
    assert_eq!(app.toast.as_ref().unwrap().kind, ToastKind::Info);
    assert_eq!(app.tree_state.cursor, vec![0]);
    assert_eq!(app.tree.get(&[0]).unwrap().name(), "exam");
    assert_eq!(app.tree.get(&[0]).unwrap().header.description, None); // left empty
}

#[tokio::test]
async fn description_from_form_is_trimmed_and_saved() {
    let tmp = tempfile::tempdir().unwrap();
    let mut t = Tree::load_from(tmp.path()).await.unwrap(); // empty root
    let mut app = App::new(&mut t, TreeState::default());

    app.handle_key(key('t')).await;
    for k in type_str("exam") {
        app.handle_key(k).await;
    }
    app.handle_key(press(KeyCode::Tab)).await; // -> description
    for k in type_str("  read ch 3 ") {
        app.handle_key(k).await;
    }
    app.handle_key(press(KeyCode::Enter)).await;

    let desc = &app.tree.get(&[0]).unwrap().header.description;
    assert_eq!(desc.as_deref(), Some("read ch 3"));
}

#[tokio::test]
async fn invalid_name_keeps_form_open_with_error() {
    let tmp = tempfile::tempdir().unwrap();
    let mut t = Tree::load_from(tmp.path()).await.unwrap();
    let mut app = App::new(&mut t, TreeState::default());

    app.handle_key(key('t')).await;
    for k in type_str("a/b") {
        app.handle_key(k).await;
    }
    app.handle_key(press(KeyCode::Enter)).await;

    assert!(matches!(app.mode, Mode::Form(_)), "form closed on error");
    assert_eq!(app.toast.as_ref().unwrap().kind, ToastKind::Error);
    assert!(app.tree.get(&[]).unwrap().children().is_empty());
}

#[tokio::test]
async fn esc_closes_form_without_creating() {
    let mut t = tree();
    let mut app = App::new(&mut t, state_at(&[0]));

    app.handle_key(key('c')).await;
    app.handle_key(key('x')).await;
    app.handle_key(press(KeyCode::Esc)).await;

    assert_eq!(app.mode, Mode::Normal);
    assert_eq!(app.tree.get(&[]).unwrap().children().len(), 2);
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
    let mut t = disk_tree(tmp.path()).await;
    let mut app = App::new(&mut t, state_at(&[1]));

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
    let mut t = disk_tree(tmp.path()).await;
    let mut app = App::new(&mut t, state_at(&[0]));

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
    let mut app = App::new(&mut t, state_at(&[0]));

    app.handle_key(key('e')).await;
    app.handle_key(press(KeyCode::Tab)).await; // -> description
    app.handle_key(clear()).await;
    app.handle_key(press(KeyCode::Enter)).await;

    assert_eq!(app.tree.get(&[0]).unwrap().header.description, None);
}

#[tokio::test]
async fn edit_to_a_siblings_name_keeps_form_open_with_error() {
    let tmp = tempfile::tempdir().unwrap();
    let mut t = disk_tree(tmp.path()).await;
    let mut app = App::new(&mut t, state_at(&[0]));

    app.handle_key(key('e')).await;
    app.handle_key(clear()).await;
    type_into(&mut app, "exam").await; // [1] is called that
    app.handle_key(press(KeyCode::Enter)).await;

    assert!(matches!(app.mode, Mode::Form(_)), "form closed on error");
    assert_eq!(app.toast.as_ref().unwrap().kind, ToastKind::Error);
    assert_eq!(app.tree.get(&[0]).unwrap().name(), "sheet");
}

#[tokio::test]
async fn edit_changes_container_kind_on_disk() {
    let tmp = tempfile::tempdir().unwrap();
    let mut t = Tree::load_from(tmp.path()).await.unwrap();
    let ws_dir = tmp.path().join("uni");
    let uni = container_at("uni", &ws_dir, ContainerKind::Workspace, vec![]);
    t.create(&[], uni).await.unwrap();
    let mut app = App::new(&mut t, state_at(&[0]));

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
    let mut t = tree_with(vec![]); // cursor on the root
    let mut app = App::new(&mut t, TreeState::default());

    app.handle_key(key('e')).await;

    assert_eq!(app.mode, Mode::Normal);
    assert_eq!(app.toast.as_ref().unwrap().kind, ToastKind::Error);
}

#[tokio::test]
async fn esc_closes_edit_form_without_changes() {
    let mut t = tree(); // in memory: Esc never saves
    let mut app = App::new(&mut t, state_at(&[0]));

    app.handle_key(key('e')).await;
    type_into(&mut app, "zzz").await;
    app.handle_key(press(KeyCode::Esc)).await;

    assert_eq!(app.mode, Mode::Normal);
    assert_eq!(app.tree.get(&[0]).unwrap().name(), "a");
}
