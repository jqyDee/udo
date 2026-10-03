use std::path::{Path, PathBuf};

use super::*;
use crate::{
    model::settings::TaskFolderSetting,
    test_util::{at, dt, parse_time, press, session, task_form},
};

fn test_form() -> Form {
    Form {
        title: "Test Form".into(),
        fields: vec![
            FormField::text(FieldId::Name, "val1"),
            FormField::text(FieldId::Dir, "val2"),
            FormField::text(FieldId::Due, "val3"),
        ],
        parent_dir: Some(PathBuf::from("/tmp/mm")),
        active_field: 0,
        action: FormAction::CreateTask { parent: vec![] },
    }
}

// --------------- Form Navigation & Constructors Tests ---------------

#[test]
fn form_next_field_advances_and_wraps() {
    let mut form = test_form();
    assert_eq!(form.active_field, 0);

    let f = form.next_field();
    assert_eq!(f.id, FieldId::Dir);
    assert_eq!(form.active_field, 1);

    let f = form.next_field();
    assert_eq!(f.id, FieldId::Due);
    assert_eq!(form.active_field, 2);

    // wraps back to 0
    let f = form.next_field();
    assert_eq!(f.id, FieldId::Name);
    assert_eq!(form.active_field, 0);
}

#[test]
fn form_prev_field_wraps_from_zero_and_steps_back() {
    let mut form = test_form();
    assert_eq!(form.active_field, 0);

    // prev from 0 wraps to last field without underflowing
    let f = form.prev_field();
    assert_eq!(f.id, FieldId::Due);
    assert_eq!(form.active_field, 2);

    let f = form.prev_field();
    assert_eq!(f.id, FieldId::Dir);
    assert_eq!(form.active_field, 1);

    let f = form.prev_field();
    assert_eq!(f.id, FieldId::Name);
    assert_eq!(form.active_field, 0);
}

#[test]
fn form_active_field_mut() {
    let mut form = test_form();
    assert_eq!(form.active_field_mut().unwrap().id, FieldId::Name);
    form.next_field();
    assert_eq!(form.active_field_mut().unwrap().id, FieldId::Dir);
}

#[test]
fn new_task_initialization() {
    let fixed_date = dt(2026, 10, 15, 14, 30);
    let defaults = TaskDefaults {
        due: fixed_date,
        folder: FolderMode::Auto,
    };
    let form = Form::new_task(vec![0], "CS101", None, defaults);

    assert_eq!(form.title, "new task · in CS101");
    assert_eq!(form.active_field, 0);
    assert_eq!(form.action, FormAction::CreateTask { parent: vec![0] });
    assert_eq!(form.text_value(FieldId::Name), Some(""));
    assert_eq!(form.text_value(FieldId::Description), Some(""));
    assert_eq!(form.date_value(FieldId::Due), Some(fixed_date));
    assert_eq!(form.folder_mode(), Some(FolderMode::Auto));
    assert_eq!(form.text_value(FieldId::Dir), Some(""));
}

#[test]
fn new_container_workspace() {
    let form = Form::new_container(vec![], "root", None, ContainerKind::Workspace);

    assert_eq!(form.title, "new container · in root");
    assert_eq!(form.active_field, 0);
    assert_eq!(form.action, FormAction::CreateContainer { parent: vec![] });
    assert_eq!(form.container_kind(), Some(ContainerKind::Workspace));
    assert_eq!(form.folder_mode(), Some(FolderMode::Auto));
    assert_eq!(form.text_value(FieldId::Name), Some(""));
    assert_eq!(form.text_value(FieldId::Description), Some(""));
    assert_eq!(form.text_value(FieldId::Dir), Some(""));
}

#[test]
fn new_container_project_auto_dir_below_parent() {
    let parent_dir = PathBuf::from("/home/user/uni");
    let form = Form::new_container(vec![1], "uni", Some(parent_dir), ContainerKind::Project);

    assert_eq!(form.title, "new container · in uni");
    assert_eq!(form.action, FormAction::CreateContainer { parent: vec![1] });
    assert_eq!(form.container_kind(), Some(ContainerKind::Project));

    let mut form = form;
    for c in "cs 101".chars() {
        form.handle_key(press(KeyCode::Char(c)));
    }
    assert_eq!(form.chosen_dir(), Ok(Some("/home/user/uni/cs_101".into())));
}

#[test]
fn container_kind_can_be_changed() {
    let mut form = Form::new_container(vec![1], "uni", None, ContainerKind::Project);
    focus(&mut form, FieldId::Kind);
    form.handle_key(press(KeyCode::Left)); // project -> workspace (nested)
    assert_eq!(form.container_kind(), Some(ContainerKind::Workspace));
}

// --------------- Edit Form Tests ---------------

fn ids(form: &Form) -> Vec<FieldId> {
    form.fields.iter().map(|f| f.id).collect()
}

#[test]
fn edit_task_is_prefilled_without_folder_rows() {
    let due = dt(2026, 10, 15, 14, 30); // local, like the form shows it
    let task = Task::new(Some("/uni/lab".into()), local_to_fixed(due).unwrap());
    let node = Node::task("lab 3".into(), task).with_description(Some("ex 1-4".into()));

    let form = Form::edit_node(vec![0, 1], &node);

    assert_eq!(form.title, "edit task · lab 3");
    assert_eq!(form.action, FormAction::EditNode { path: vec![0, 1] });
    assert_eq!(ids(&form), [FieldId::Name, FieldId::Description, FieldId::Due]);
    assert_eq!(form.name(), "lab 3");
    assert_eq!(form.description(), "ex 1-4");
    assert_eq!(form.date_value(FieldId::Due), Some(due)); // stored -> local round trip
    assert_eq!(form.container_kind(), None);
    assert_eq!(form.chosen_dir(), Ok(None)); // no folder rows: dir untouched
}

#[test]
fn edit_container_is_prefilled_with_its_kind() {
    let c = Container::new("/uni".into(), ContainerKind::Project);
    let form = Form::edit_node(vec![0], &Node::container("uni".into(), c));

    assert_eq!(form.title, "edit container · uni");
    assert_eq!(ids(&form), [FieldId::Name, FieldId::Description, FieldId::Kind]);
    assert_eq!(form.description(), ""); // none set
    assert_eq!(form.container_kind(), Some(ContainerKind::Project));
    assert_eq!(form.due(), Ok(None));
}

#[test]
fn create_and_edit_forms_share_field_order() {
    let defaults = TaskDefaults {
        due: dt(2026, 10, 15, 14, 30),
        folder: FolderMode::Auto,
    };
    let create = Form::new_task(vec![], "root", None, defaults);
    let edit = Form::edit_node(vec![0], &Node::task("t".into(), Task::new(None, now())));
    // edit = create without the folder rows
    let without_folder: Vec<_> = ids(&create)
        .into_iter()
        .filter(|id| !matches!(id, FieldId::Folder | FieldId::Dir))
        .collect();
    assert_eq!(ids(&edit), without_folder);
}

#[test]
fn name_is_normalized_but_the_description_kept_raw() {
    let mut form = Form::edit_node(vec![0], &Node::task("a".into(), Task::new(None, now())));
    focus(&mut form, FieldId::Name);
    for c in "  b   c ".chars() {
        form.handle_key(press(KeyCode::Char(c)));
    }
    focus(&mut form, FieldId::Description);
    for c in "  d ".chars() {
        form.handle_key(press(KeyCode::Char(c)));
    }
    assert_eq!(form.name(), "a b c"); // collapsed like everywhere
    assert_eq!(form.description(), "  d "); // the tree cleans it
}

// --------------- Date Field Tests ---------------

#[test]
fn form_field_date_wraps_date_input() {
    let f = FormField::date(FieldId::Due, dt(2026, 10, 15, 14, 30));
    assert_eq!(f.id, FieldId::Due);
    assert_eq!(f.input, FieldInput::Date(DateInput::new(dt(2026, 10, 15, 14, 30))));
}

#[test]
fn value_getters_only_match_their_own_kind() {
    let defaults = TaskDefaults {
        due: dt(2026, 10, 15, 14, 30),
        folder: FolderMode::Auto,
    };
    let form = Form::new_task(vec![0], "CS101", None, defaults);
    assert_eq!(form.text_value(FieldId::Due), None); // due is a date
    assert_eq!(form.date_value(FieldId::Name), None); // name is text
    assert_eq!(form.date_value(FieldId::Dir), None); // dir is text
    assert_eq!(form.text_value(FieldId::Folder), None); // folder is a choice
    assert_eq!(form.choice_value(FieldId::Name), None);
    assert_eq!(form.choice_value(FieldId::Folder), Some(FolderMode::Auto.index()));
}

// --------------- Key Tests ---------------

#[test]
fn handle_key_outcomes() {
    let mut form = test_form();
    assert_eq!(form.handle_key(press(KeyCode::Enter)), FormOutcome::Submit);
    assert_eq!(form.handle_key(press(KeyCode::Esc)), FormOutcome::Cancel);
    assert_eq!(form.handle_key(press(KeyCode::Char('x'))), FormOutcome::Continue);
}

#[test]
fn handle_key_tab_switches_and_other_keys_edit_active_field() {
    let mut form = test_form();
    form.handle_key(press(KeyCode::Tab));
    assert_eq!(form.active_field, 1);
    form.handle_key(press(KeyCode::Char('!')));
    assert_eq!(form.text_value(FieldId::Dir), Some("val2!"));
    assert_eq!(form.text_value(FieldId::Name), Some("val1")); // untouched

    form.handle_key(press(KeyCode::BackTab));
    assert_eq!(form.active_field, 0);
}

#[test]
fn handle_key_reaches_date_fields() {
    let defaults = TaskDefaults {
        due: dt(2026, 6, 15, 12, 0),
        folder: FolderMode::Auto,
    };
    let mut form = Form::new_task(vec![], "root", None, defaults);
    focus(&mut form, FieldId::Due); // starts on Day
    form.handle_key(press(KeyCode::Up));
    assert_eq!(form.date_value(FieldId::Due), Some(dt(2026, 6, 16, 12, 0)));
}

// --------------- Folder / Dir Tests ---------------

fn active_id(form: &Form) -> FieldId {
    form.fields[form.active_field].id
}

/// Press Tab until `id` is active, so tests don't depend on field order.
fn focus(form: &mut Form, id: FieldId) {
    for _ in 0..form.fields.len() {
        if active_id(form) == id {
            return;
        }
        form.handle_key(press(KeyCode::Tab));
    }
    panic!("Tab never reached {id:?}");
}

#[test]
fn auto_preview_follows_the_name() {
    let form = task_form(FolderMode::Auto, "lab 3");
    assert_eq!(form.dir_preview(), Some("/uni/cs101/lab_3".into()));
    assert_eq!(task_form(FolderMode::Auto, "").dir_preview(), None);
}

#[test]
fn none_has_no_dir_and_custom_uses_the_text() {
    assert_eq!(task_form(FolderMode::None, "lab 3").dir_preview(), None);

    let mut form = task_form(FolderMode::Custom, "lab 3");
    assert_eq!(form.dir_preview(), None); // nothing typed yet
    focus(&mut form, FieldId::Dir);
    for c in "~/x".chars() {
        form.handle_key(press(KeyCode::Char(c)));
    }
    assert_eq!(form.dir_preview(), Some("~/x".into()));
}

#[test]
fn container_folder_offers_no_none() {
    let mut form = Form::new_container(vec![], "root", None, ContainerKind::Workspace);
    focus(&mut form, FieldId::Folder);
    let mut seen = vec![];
    for _ in 0..CONTAINER_FOLDER_CHOICES.len() {
        seen.push(form.folder_mode().unwrap());
        form.handle_key(press(KeyCode::Right));
    }
    assert_eq!(seen, [FolderMode::Auto, FolderMode::Custom]);
    assert_eq!(form.folder_mode(), Some(FolderMode::Auto)); // wrapped
}

#[test]
fn chosen_dir_checks_custom_text() {
    let mut form = task_form(FolderMode::Auto, "lab 3");
    focus(&mut form, FieldId::Folder);
    form.handle_key(press(KeyCode::Right)); // custom, prefilled with auto path
    assert_eq!(form.chosen_dir(), Ok(Some("/uni/cs101/lab_3".into())));

    // replace the text with a relative path
    let dir = form.fields.iter_mut().find(|f| f.id == FieldId::Dir);
    let Some(FormField {
        input: FieldInput::Text(t),
        ..
    }) = dir
    else {
        panic!("dir is not a text field");
    };
    *t = TextInput::new("relative/dir");
    assert!(form.chosen_dir().is_err());

    assert_eq!(task_form(FolderMode::None, "x").chosen_dir(), Ok(None));
}

#[test]
fn switching_to_custom_prefills_the_auto_path() {
    let mut form = task_form(FolderMode::Auto, "lab 3");
    focus(&mut form, FieldId::Folder);
    form.handle_key(press(KeyCode::Right)); // auto -> custom
    assert_eq!(form.folder_mode(), Some(FolderMode::Custom));
    assert_eq!(form.text_value(FieldId::Dir), Some("/uni/cs101/lab_3"));
}

#[test]
fn prefill_without_name_starts_at_the_parent_dir() {
    let mut form = task_form(FolderMode::Auto, "");
    focus(&mut form, FieldId::Folder);
    form.handle_key(press(KeyCode::Right));
    assert_eq!(form.text_value(FieldId::Dir), Some("/uni/cs101/"));
}

#[test]
fn custom_text_is_kept_while_switching_modes() {
    let mut form = task_form(FolderMode::Auto, "lab 3");
    focus(&mut form, FieldId::Folder);
    form.handle_key(press(KeyCode::Right)); // custom, prefilled
    form.handle_key(press(KeyCode::Tab)); // dir
    form.handle_key(press(KeyCode::Char('!')));
    form.handle_key(press(KeyCode::BackTab)); // folder
    form.handle_key(press(KeyCode::Left)); // auto
    assert_eq!(form.dir_preview(), Some("/uni/cs101/lab_3".into()));
    form.handle_key(press(KeyCode::Right)); // custom again
    assert_eq!(form.text_value(FieldId::Dir), Some("/uni/cs101/lab_3!"));
}

#[test]
fn tab_skips_the_dir_row_outside_custom() {
    for mode in [FolderMode::Auto, FolderMode::None] {
        let mut form = task_form(mode, "");
        form.handle_key(press(KeyCode::Tab));
        assert_eq!(active_id(&form), FieldId::Description, "{mode:?}");
        form.handle_key(press(KeyCode::Tab));
        assert_eq!(active_id(&form), FieldId::Folder, "{mode:?}");
        form.handle_key(press(KeyCode::Tab));
        assert_eq!(active_id(&form), FieldId::Due, "{mode:?}");
        form.handle_key(press(KeyCode::BackTab));
        assert_eq!(active_id(&form), FieldId::Folder, "{mode:?}");
    }
}

#[test]
fn tab_reaches_the_dir_row_in_custom() {
    let mut form = task_form(FolderMode::Custom, "");
    focus(&mut form, FieldId::Folder);
    form.handle_key(press(KeyCode::Tab));
    assert_eq!(active_id(&form), FieldId::Dir);
    form.handle_key(press(KeyCode::Tab));
    assert_eq!(active_id(&form), FieldId::Due);
}

// --------------- Settings Form Tests ---------------

/// Own: deadline only. Placeholder: `<label> (inherited)`.
fn settings_form(root: Option<&RootSettings>) -> Form {
    let own = ContainerSettings {
        default_deadline: Some("fri 22:00".parse().unwrap()),
        ..Default::default()
    };
    Form::edit_settings(vec![0], "uni", &own, |info| format!("{} (inherited)", info.label), root)
}

/// What a setting field says applies when unset: the placeholder of a text
/// field, the hint of a choice.
fn unset_hint(form: &Form, id: FieldId) -> Option<&str> {
    match &form.fields.iter().find(|f| f.id == id)?.input {
        FieldInput::Text(t) => t.placeholder.as_deref(),
        FieldInput::Choice(c) => c.hint.as_deref(),
        FieldInput::Date(_) => None,
    }
}

/// A setting field's value as `set` gets it ("" = unset).
fn setting_text(form: &Form, id: FieldId) -> &str {
    match &form.fields.iter().find(|f| f.id == id).unwrap().input {
        FieldInput::Text(t) => &t.value,
        FieldInput::Choice(c) => c.value().unwrap_or(""),
        FieldInput::Date(_) => panic!("{id:?} is a date"),
    }
}

fn setting_id(key: &str) -> FieldId {
    FieldId::Setting(SETTINGS.iter().position(|i| i.key == key).unwrap())
}

/// Replace the text of field `id` by typing, like a user would.
fn retype(form: &mut Form, id: FieldId, text: &str) {
    focus(form, id);
    let old = form.text_value(id).unwrap().chars().count();
    for _ in 0..old {
        form.handle_key(press(KeyCode::Backspace));
    }
    for c in text.chars() {
        form.handle_key(press(KeyCode::Char(c)));
    }
}

#[test]
fn settings_form_has_one_field_per_setting_in_order() {
    let form = settings_form(None);

    let ids: Vec<_> = form.fields.iter().map(|f| f.id).collect();
    let expected: Vec<_> = (0..SETTINGS.len()).map(FieldId::Setting).collect();
    assert_eq!(ids, expected);
    assert_eq!(form.title, "settings · uni");
    assert_eq!(form.action, FormAction::EditSettings { path: vec![0] });
}

#[test]
fn settings_form_shows_own_values_and_inherited_placeholders() {
    let form = settings_form(None);

    for (i, info) in SETTINGS.iter().enumerate() {
        let id = FieldId::Setting(i);
        assert_eq!(id.label(), info.label);
        let expected = format!("{} (inherited)", info.label);
        assert_eq!(unset_hint(&form, id), Some(expected.as_str()));
        let text = setting_text(&form, id);
        match info.key {
            "default_deadline" => assert_eq!(text, "fri 22:00"),
            _ => assert_eq!(text, "", "{}", info.key), // not set: inherit
        }
    }
}

#[test]
fn settings_form_unchanged_gives_the_same_settings() {
    let (settings, root) = settings_form(None).settings().unwrap();

    assert_eq!(
        settings,
        ContainerSettings {
            default_deadline: Some("fri 22:00".parse().unwrap()),
            ..Default::default()
        }
    );
    assert_eq!(root, None); // not the root: no root settings sent
}

#[test]
fn settings_form_typed_and_cleared_values() {
    let mut form = settings_form(None);

    retype(&mut form, setting_id("default_deadline"), ""); // cleared: inherit again
    retype(&mut form, setting_id("archive_dir"), "/arch");

    let (settings, _) = form.settings().unwrap();
    assert_eq!(settings.default_deadline, None);
    assert_eq!(settings.archive_dir, Some(PathBuf::from("/arch")));
}

#[test]
fn settings_with_choices_get_a_choice_with_inherit_first() {
    let form = settings_form(None);

    for (i, info) in SETTINGS.iter().enumerate() {
        let input = &form.fields[i].input;
        if info.choices.is_empty() {
            assert!(matches!(input, FieldInput::Text(_)), "{}", info.key);
            continue;
        }
        let FieldInput::Choice(c) = input else {
            panic!("{} has choices but no choice field", info.key);
        };
        assert_eq!(c.options[0], "inherit");
        assert_eq!(&c.options[1..], info.choices);
    }
}

#[test]
fn task_folders_choice_starts_on_the_own_value_or_inherit() {
    let id = setting_id("task_folders");
    let unset = settings_form(None);
    assert_eq!(setting_text(&unset, id), ""); // inherit

    let own = ContainerSettings {
        task_folders: Some(TaskFolderSetting::None),
        ..Default::default()
    };
    let set = Form::edit_settings(vec![0], "uni", &own, |_| String::new(), None);
    assert_eq!(setting_text(&set, id), "none");
}

#[test]
fn task_folders_choice_is_read_back() {
    let mut form = settings_form(None);
    focus(&mut form, setting_id("task_folders"));

    form.handle_key(press(KeyCode::Right)); // inherit -> auto
    assert_eq!(form.settings().unwrap().0.task_folders, Some(TaskFolderSetting::Auto));

    form.handle_key(press(KeyCode::Left)); // back to inherit
    assert_eq!(form.settings().unwrap().0.task_folders, None);
}

#[test]
fn settings_form_bad_input_names_the_setting() {
    let mut form = settings_form(None);

    retype(&mut form, setting_id("default_deadline"), "someday");

    let err = form.settings().unwrap_err();
    assert!(err.starts_with("deadline: "), "got: {err}");
}

#[test]
fn settings_form_on_the_root_adds_root_settings() {
    let root = RootSettings {
        theme: Some("dark".into()),
        ..Default::default()
    };
    let mut form = settings_form(Some(&root));

    let root_ids: Vec<_> = form
        .fields
        .iter()
        .map(|f| f.id)
        .filter(|id| matches!(id, FieldId::RootSetting(_)))
        .collect();
    let expected: Vec<_> = (0..ROOT_SETTINGS.len()).map(FieldId::RootSetting).collect();
    assert_eq!(root_ids, expected);
    // after the container settings
    assert_eq!(form.fields[SETTINGS.len()].id, FieldId::RootSetting(0));

    let theme = FieldId::RootSetting(0);
    assert_eq!(form.text_value(theme), Some("dark"));
    assert_eq!(unset_hint(&form, theme), Some("not set"));
    assert_eq!(form.settings().unwrap().1, Some(root));

    retype(&mut form, theme, ""); // cleared: still sent, as unset
    assert_eq!(form.settings().unwrap().1, Some(RootSettings::default()));
}

// --------------- Readers ---------------

/// Put `input` straight into the field `id` (dates the keys can't reach,
/// like a DST gap; text without typing it).
fn set_input(form: &mut Form, id: FieldId, input: FieldInput) {
    let field = form.fields.iter_mut().find(|f| f.id == id);
    field.expect("no such field").input = input;
}

/// Local 2:30 on the day clocks spring forward, if this zone skips it.
fn dst_gap() -> Option<NaiveDateTime> {
    // only testable where the local zone has a gap then (e.g. Europe)
    let gap = dt(2026, 3, 29, 2, 30);
    local_to_fixed(gap).is_none().then_some(gap)
}

#[test]
fn new_task_node_gives_the_task_to_create() {
    let node = task_form(FolderMode::None, "lab 3")
        .new_task_node()
        .unwrap();

    assert_eq!(node.name(), "lab 3");
    assert_eq!(node.dir(), None);
    // `task_form`'s default due, local
    let due = local_to_fixed(dt(2026, 6, 15, 12, 0)).unwrap();
    assert_eq!(node.as_task().unwrap().due_date, due);
}

#[test]
fn a_due_date_skipped_by_dst_is_refused() {
    let Some(gap) = dst_gap() else { return };
    let mut form = task_form(FolderMode::None, "lab 3");
    set_input(&mut form, FieldId::Due, FieldInput::Date(DateInput::new(gap)));

    // `.err()`: a `Node` can't be printed, so no `unwrap_err`
    let err = form.new_task_node().err().expect("a DST gap is refused");
    assert!(err.contains("DST"), "{err}");
    assert!(form.node_edit().is_err());
}

#[test]
fn a_relative_custom_dir_is_refused() {
    let mut form = task_form(FolderMode::Custom, "lab 3");
    set_input(&mut form, FieldId::Dir, FieldInput::Text(TextInput::new("relative/dir")));

    assert!(form.new_task_node().is_err());
}

#[test]
fn new_container_node_gets_the_auto_dir_and_kind() {
    let mut form =
        Form::new_container(vec![], "root", Some("/uni".into()), ContainerKind::Workspace);
    for c in "cs 101".chars() {
        form.handle_key(press(KeyCode::Char(c)));
    }

    let node = form.new_container_node().unwrap();

    assert_eq!(node.name(), "cs 101");
    assert_eq!(node.dir(), Some(Path::new("/uni/cs_101")));
    assert_eq!(node.as_container().unwrap().kind, ContainerKind::Workspace);
}

#[test]
fn auto_container_without_a_name_is_refused() {
    let form = Form::new_container(vec![], "root", Some("/uni".into()), ContainerKind::Workspace);

    assert_eq!(form.new_container_node().err().as_deref(), Some("name cannot be empty"));
}

#[test]
fn node_edit_sends_every_field() {
    let due = dt(2026, 10, 15, 14, 30);
    let task = Task::new(None, local_to_fixed(due).unwrap());
    let form = Form::edit_node(vec![0], &Node::task("lab 3".into(), task));

    let patch = form.node_edit().unwrap();

    assert_eq!(patch.header.name.as_deref(), Some("lab 3"));
    // blank: sent anyway, the tree removes it
    assert_eq!(patch.header.description, Some(Some(String::new())));
    let Some(BodyPatch::Task(t)) = patch.body else {
        panic!("a task form patches the task");
    };
    assert_eq!(t.due_date, local_to_fixed(due));
}

#[test]
fn node_edit_of_a_container_patches_its_kind() {
    let c = Container::new("/uni".into(), ContainerKind::Project);
    let form = Form::edit_node(vec![0], &Node::container("uni".into(), c));

    let Some(BodyPatch::Container(c)) = form.node_edit().unwrap().body else {
        panic!("a container form patches the container");
    };
    assert_eq!(c.kind, Some(ContainerKind::Project));
}

// --------------- Session form ---------------

#[test]
fn session_form_has_start_and_end_in_local_time() {
    let s = session(at(9, 0), Some(at(10, 30)));

    let form = Form::edit_session(&s);

    // shown local, read back as the same instants
    assert_eq!(form.session_times(), Ok((at(9, 0), Some(at(10, 30)))));
    assert_eq!(form.action, FormAction::EditSession { id: s.id });
    assert_eq!(form.title, "edit session · lab 3");
}

#[test]
fn a_running_session_has_no_end_field() {
    let form = Form::edit_session(&session(at(9, 0), None));

    assert_eq!(form.fields.len(), 1);
    assert_eq!(form.session_times(), Ok((at(9, 0), None)));
    assert_eq!(form.title, "edit session · lab 3 (running)");
}

#[test]
fn session_form_starts_on_the_start_and_tabs_to_the_end() {
    let mut form = Form::edit_session(&session(at(9, 0), Some(at(10, 0))));
    assert_eq!(form.fields[form.active_field].id, FieldId::Start);

    form.handle_key(press(KeyCode::Tab));
    assert_eq!(form.fields[form.active_field].id, FieldId::End);
    form.handle_key(press(KeyCode::Tab));
    assert_eq!(form.fields[form.active_field].id, FieldId::Start); // wraps
}

/// Like the due field: ←/→ pick the segment, ↑/↓ change it; minutes by
/// `SESSION_MINUTE_STEP` (1), not the due field's 5.
#[test]
fn keys_change_the_start_by_single_minutes() {
    let mut form = Form::edit_session(&session(at(9, 0), Some(at(10, 0))));

    form.handle_key(press(KeyCode::Right)); // day -> hour
    form.handle_key(press(KeyCode::Right)); // -> minute
    form.handle_key(press(KeyCode::Up));

    // end untouched
    assert_eq!(form.session_times(), Ok((at(9, 1), Some(at(10, 0)))));
}

/// Enter / Esc end the form like every other form.
#[test]
fn enter_submits_and_esc_cancels() {
    let mut form = Form::edit_session(&session(at(9, 0), Some(at(10, 0))));

    assert_eq!(form.handle_key(press(KeyCode::Enter)), FormOutcome::Submit);
    assert_eq!(form.handle_key(press(KeyCode::Esc)), FormOutcome::Cancel);
}

/// The value keeps the seconds of the session (the field shows minutes
/// only), also after a step: "unchanged" can compare values directly.
#[test]
fn seconds_are_kept_also_after_a_step() {
    let start = parse_time("2026-10-15T14:00:40+02:00");
    let mut form = Form::edit_session(&session(start, Some(at(15, 0))));

    assert_eq!(form.session_times().unwrap().0, start); // untouched

    form.handle_key(press(KeyCode::Right));
    form.handle_key(press(KeyCode::Right)); // minute
    form.handle_key(press(KeyCode::Up));

    let stepped = start + chrono::TimeDelta::minutes(1);
    assert_eq!(form.session_times().unwrap().0, stepped); // 14:01:40
}

// --------------- Split / cut forms ---------------

/// "now" for the split / cut forms: after every test session.
fn now() -> Time {
    at(20, 0)
}

#[test]
fn split_starts_on_the_midpoint() {
    let s = session(at(9, 0), Some(at(10, 0)));

    let form = Form::split_session(&s, now());

    assert_eq!(form.split_at(), Ok(at(9, 30)));
    assert_eq!(form.action, FormAction::SplitSession { id: s.id });
    assert_eq!(form.title, "split session · lab 3");
}

/// 9:00–10:31: the midpoint is 9:45:30, the form starts on 9:45.
#[test]
fn split_midpoint_is_rounded_down_to_the_minute() {
    let form = Form::split_session(&session(at(9, 0), Some(at(10, 31))), now());

    assert_eq!(form.split_at(), Ok(at(9, 45)));
}

/// Seconds from the timer: 14:00:40–15:00:00, midpoint 14:30:20 -> 14:30.
#[test]
fn split_drops_the_seconds_of_the_session() {
    let start = parse_time("2026-10-15T14:00:40+02:00");
    let form = Form::split_session(&session(start, Some(at(15, 0))), now());

    assert_eq!(form.split_at(), Ok(at(14, 30)));
}

/// Not offered by the app, but no panic: the midpoint up to `now`.
#[test]
fn split_on_a_running_session_uses_now() {
    let form = Form::split_session(&session(at(9, 0), None), at(9, 40));

    assert_eq!(form.split_at(), Ok(at(9, 20)));
    assert_eq!(form.title, "split session · lab 3 (running)");
}

#[test]
fn cut_starts_on_the_midpoint_and_lasts_30_minutes() {
    let s = session(at(9, 0), Some(at(12, 0)));

    let form = Form::cut_session(&s, now());

    assert_eq!(form.cut_range(), Ok((at(10, 30), at(11, 0))));
    assert_eq!(form.action, FormAction::CutSession { id: s.id });
    assert_eq!(form.title, "cut session · lab 3");
    assert_eq!(active_id(&form), FieldId::From);
}

/// Shorter than 30 minutes after the midpoint: up to the end, still valid.
#[test]
fn cut_is_capped_at_the_end() {
    let form = Form::cut_session(&session(at(9, 0), Some(at(9, 40))), now());

    assert_eq!(form.cut_range(), Ok((at(9, 20), at(9, 40))));
}

/// Running: the midpoint up to `now`, capped at `now`.
#[test]
fn cut_on_a_running_session_is_capped_at_now() {
    let form = Form::cut_session(&session(at(9, 0), None), at(9, 40));

    assert_eq!(form.cut_range(), Ok((at(9, 20), at(9, 40))));
    assert_eq!(form.title, "cut session · lab 3 (running)");
}

/// Running, now 9:00:50: the midpoint 9:00:25 and `to` (now) both round
/// down to 9:00. An empty cut: `Core` refuses it on save.
#[test]
fn cut_defaults_are_rounded_down_to_the_minute() {
    let now = parse_time("2026-10-15T09:00:50+02:00");
    let form = Form::cut_session(&session(at(9, 0), None), now);

    assert_eq!(form.cut_range(), Ok((at(9, 0), at(9, 0))));
}

/// Like the edit form: ↑ on the minute segment moves one minute.
#[test]
fn keys_move_the_cut_by_single_minutes() {
    let mut form = Form::cut_session(&session(at(9, 0), Some(at(12, 0))), now());

    form.handle_key(press(KeyCode::Right)); // day -> hour
    form.handle_key(press(KeyCode::Right)); // -> minute
    form.handle_key(press(KeyCode::Up)); // from 10:31
    form.handle_key(press(KeyCode::Tab)); // to `to`, on its day segment
    form.handle_key(press(KeyCode::Right));
    form.handle_key(press(KeyCode::Right));
    form.handle_key(press(KeyCode::Down)); // to 10:59

    assert_eq!(form.cut_range(), Ok((at(10, 31), at(10, 59))));
}

#[test]
fn split_at_and_cut_range_refuse_a_dst_gap() {
    let Some(gap) = dst_gap() else { return };
    let s = session(at(9, 0), Some(at(12, 0)));
    let mut split = Form::split_session(&s, now());
    let mut cut = Form::cut_session(&s, now());

    set_input(&mut split, FieldId::At, FieldInput::Date(DateInput::new(gap)));
    set_input(&mut cut, FieldId::To, FieldInput::Date(DateInput::new(gap)));

    let err = split.split_at().expect_err("a DST gap is refused");
    assert!(err.contains("DST"), "{err}");
    let err = cut.cut_range().expect_err("a DST gap is refused");
    assert!(err.contains("DST"), "{err}");
}

// --------------- Add form ---------------

/// The last hour up to now, rounded down: now 14:00:40 -> 13:00–14:00.
#[test]
fn add_starts_on_the_last_hour() {
    let now = parse_time("2026-10-15T14:00:40+02:00");

    let form = Form::add_session("lab 3", vec![0, 1], now);

    assert_eq!(form.add_times(), Ok((at(13, 0), at(14, 0))));
    assert_eq!(form.action, FormAction::AddSession { path: vec![0, 1] });
    assert_eq!(form.title, "add session · lab 3");
    assert_eq!(active_id(&form), FieldId::Start);
}

/// Like the other session forms: ↑ on the minute segment moves one minute.
#[test]
fn keys_move_the_add_times_by_single_minutes() {
    let mut form = Form::add_session("lab 3", vec![0], at(14, 0));

    form.handle_key(press(KeyCode::Right)); // day -> hour
    form.handle_key(press(KeyCode::Right)); // -> minute
    form.handle_key(press(KeyCode::Up)); // start 13:01
    form.handle_key(press(KeyCode::Tab)); // to `end`, on its day segment
    form.handle_key(press(KeyCode::Right));
    form.handle_key(press(KeyCode::Right));
    form.handle_key(press(KeyCode::Down)); // end 13:59

    assert_eq!(form.add_times(), Ok((at(13, 1), at(13, 59))));
}

#[test]
fn add_times_refuse_a_dst_gap() {
    let Some(gap) = dst_gap() else { return };
    let mut form = Form::add_session("lab 3", vec![0], now());

    set_input(&mut form, FieldId::End, FieldInput::Date(DateInput::new(gap)));

    let err = form.add_times().expect_err("a DST gap is refused");
    assert!(err.contains("DST"), "{err}");
}
