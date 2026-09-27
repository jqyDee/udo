use std::path::PathBuf;

use chrono::NaiveDate;

use super::*;
use crate::{model::settings::TaskFolderSetting, test_util::press};

fn dt(y: i32, mo: u32, d: u32, h: u32, mi: u32) -> NaiveDateTime {
    NaiveDate::from_ymd_opt(y, mo, d)
        .unwrap()
        .and_hms_opt(h, mi, 0)
        .unwrap()
}

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
    let fixed_date = NaiveDate::from_ymd_opt(2026, 10, 15)
        .unwrap()
        .and_hms_opt(14, 30, 0)
        .unwrap();
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
    assert_eq!(
        ids(&form),
        [FieldId::Name, FieldId::Description, FieldId::Due]
    );
    let v = form.values();
    assert_eq!(v.name, "lab 3");
    assert_eq!(v.description, "ex 1-4");
    assert_eq!(v.due, Some(due)); // stored -> local round trip
    assert_eq!(v.kind, None);
    assert_eq!(form.chosen_dir(), Ok(None)); // no folder rows: dir untouched
}

#[test]
fn edit_container_is_prefilled_with_its_kind() {
    let c = Container::new("/uni".into(), ContainerKind::Project);
    let form = Form::edit_node(vec![0], &Node::container("uni".into(), c));

    assert_eq!(form.title, "edit container · uni");
    assert_eq!(
        ids(&form),
        [FieldId::Name, FieldId::Description, FieldId::Kind]
    );
    let v = form.values();
    assert_eq!(v.description, ""); // none set
    assert_eq!(v.kind, Some(ContainerKind::Project));
    assert_eq!(v.due, None);
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
fn values_normalize_the_name_but_keep_the_description_raw() {
    let mut form = Form::edit_node(vec![0], &Node::task("a".into(), Task::new(None, now())));
    focus(&mut form, FieldId::Name);
    for c in "  b   c ".chars() {
        form.handle_key(press(KeyCode::Char(c)));
    }
    focus(&mut form, FieldId::Description);
    for c in "  d ".chars() {
        form.handle_key(press(KeyCode::Char(c)));
    }
    let v = form.values();
    assert_eq!(v.name, "a b c"); // collapsed like everywhere
    assert_eq!(v.description, "  d "); // the tree cleans it
}

// --------------- Date Field Tests ---------------

#[test]
fn form_field_date_wraps_date_input() {
    let f = FormField::date(FieldId::Due, dt(2026, 10, 15, 14, 30));
    assert_eq!(f.id, FieldId::Due);
    assert_eq!(
        f.input,
        FieldInput::Date(DateInput::new(dt(2026, 10, 15, 14, 30)))
    );
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
    assert_eq!(
        form.choice_value(FieldId::Folder),
        Some(FolderMode::Auto.index())
    );
}

// --------------- Key Tests ---------------

#[test]
fn handle_key_outcomes() {
    let mut form = test_form();
    assert_eq!(form.handle_key(press(KeyCode::Enter)), FormOutcome::Submit);
    assert_eq!(form.handle_key(press(KeyCode::Esc)), FormOutcome::Cancel);
    assert_eq!(
        form.handle_key(press(KeyCode::Char('x'))),
        FormOutcome::Continue
    );
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

/// Task form in `/uni/cs101` with `mode`, name typed in.
fn task_form(mode: FolderMode, name: &str) -> Form {
    let defaults = TaskDefaults {
        due: dt(2026, 6, 15, 12, 0),
        folder: mode,
    };
    let mut form = Form::new_task(vec![0], "cs101", Some("/uni/cs101".into()), defaults);
    for c in name.chars() {
        form.handle_key(press(KeyCode::Char(c)));
    }
    form
}

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
    Form::edit_settings(
        vec![0],
        "uni",
        &own,
        |info| format!("{} (inherited)", info.label),
        root,
    )
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
    assert_eq!(
        form.settings().unwrap().0.task_folders,
        Some(TaskFolderSetting::Auto)
    );

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
