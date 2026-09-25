//! Forms for creating nodes: a list of fields (text or date) plus what to
//! do on submit.
//!
//! - `text`: `TextInput`, free text with a cursor
//! - `date`: `DateInput`, local date + time edited by segment

mod date;
mod text;

use std::path::PathBuf;

use chrono::{Local, NaiveDateTime, NaiveTime, TimeDelta};
use crossterm::event::{KeyCode, KeyEvent};

pub use date::{DATE_FMT, DateInput, Segment, local_to_utc};
pub use text::TextInput;

use crate::model::{container::ContainerKind, tree::NodePath};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormField {
    pub label: &'static str,
    pub input: FieldInput,
}

/// What kind of value a field holds, and so which keys edit it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FieldInput {
    Text(TextInput),
    Date(DateInput),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FormAction {
    CreateTask {
        parent: NodePath,
    },
    CreateContainer {
        parent: NodePath,
        kind: ContainerKind,
    },
    EditNode {
        path: NodePath,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Form {
    pub title: String,
    pub fields: Vec<FormField>,
    pub active_field: usize,
    pub action: FormAction,
}

/// What the app should do after a key went into the form.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormOutcome {
    /// Keep editing.
    Continue,
    /// Enter: validate and create (`App::submit_form`).
    Submit,
    /// Esc: close without creating.
    Cancel,
}

impl FormField {
    /// Text field, cursor at the end of `value`.
    pub fn text(label: &'static str, value: impl Into<String>) -> Self {
        Self {
            label,
            input: FieldInput::Text(TextInput::new(value)),
        }
    }

    /// Date field, starting on `Segment::Day` (the part changed most).
    pub fn date(label: &'static str, value: NaiveDateTime) -> Self {
        Self {
            label,
            input: FieldInput::Date(DateInput::new(value)),
        }
    }
}

impl Form {
    /// Due defaults to tomorrow 12:00 local, so a new task isn't overdue.
    pub fn new_task(parent: NodePath, parent_name: &str) -> Self {
        let tomorrow_noon = (Local::now().date_naive() + TimeDelta::days(1))
            .and_time(NaiveTime::from_hms_opt(12, 0, 0).unwrap());
        Self::new_task_with_due(parent, parent_name, tomorrow_noon)
    }

    /// `due` is local time (see `DateInput`).
    pub fn new_task_with_due(parent: NodePath, parent_name: &str, due: NaiveDateTime) -> Self {
        Self {
            title: format!("new task · in {parent_name}"),
            fields: vec![FormField::text("name", ""), FormField::date("due", due)],
            active_field: 0,
            action: FormAction::CreateTask { parent },
        }
    }

    pub fn new_container(
        parent: NodePath,
        parent_name: &str,
        default_dir: Option<PathBuf>,
        kind: ContainerKind,
    ) -> Self {
        // Empty dir = `<parent dir>/<name>` (see `App::submit_form`). Not
        // prefilled: the parent dir itself already holds a `.udo.toml`.
        let dir = TextInput::new("");
        let dir = match default_dir {
            Some(d) => dir.with_placeholder(format!("{}/<name>", d.display())),
            None => dir,
        };
        Self {
            title: format!("new {:?} · in {parent_name}", kind),
            fields: vec![
                FormField::text("name", ""),
                FormField {
                    label: "dir",
                    input: FieldInput::Text(dir),
                },
            ],
            active_field: 0,
            action: FormAction::CreateContainer { parent, kind },
        }
    }

    pub fn active_field_mut(&mut self) -> Option<&mut FormField> {
        self.fields.get_mut(self.active_field)
    }

    pub fn next_field(&mut self) -> &mut FormField {
        let field_count = self.fields.len();
        assert!(field_count > 0, "Form has no fields");
        self.active_field = (self.active_field + 1) % field_count;
        &mut self.fields[self.active_field]
    }

    pub fn prev_field(&mut self) -> &mut FormField {
        let field_count = self.fields.len();
        assert!(field_count > 0, "Form has no fields");
        self.active_field = (self.active_field + field_count - 1) % field_count;
        &mut self.fields[self.active_field]
    }

    /// Tab / Shift+Tab switch fields, Enter / Esc end the form, every other
    /// key edits the active field (by its kind).
    pub fn handle_key(&mut self, key: KeyEvent) -> FormOutcome {
        match key.code {
            KeyCode::Esc => return FormOutcome::Cancel,
            KeyCode::Enter => return FormOutcome::Submit,
            KeyCode::Tab => {
                self.next_field();
            }
            KeyCode::BackTab => {
                self.prev_field();
            }
            _ => match self.active_field_mut().map(|f| &mut f.input) {
                Some(FieldInput::Text(t)) => t.handle_key(key),
                Some(FieldInput::Date(d)) => d.handle_key(key),
                None => {}
            },
        }
        FormOutcome::Continue
    }

    /// Value of the text field `label`. None if missing or not a text field.
    pub fn text_value(&self, label: &str) -> Option<&str> {
        match &self.fields.iter().find(|f| f.label == label)?.input {
            FieldInput::Text(t) => Some(t.value.as_str()),
            FieldInput::Date(_) => None,
        }
    }

    /// Value of the date field `label`. None if missing or not a date field.
    pub fn date_value(&self, label: &str) -> Option<NaiveDateTime> {
        match &self.fields.iter().find(|f| f.label == label)?.input {
            FieldInput::Date(d) => Some(d.value),
            FieldInput::Text(_) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use chrono::NaiveDate;

    use super::*;
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
                FormField::text("Field1", "val1"),
                FormField::text("Field2", "val2"),
                FormField::text("Field3", "val3"),
            ],
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
        assert_eq!(f.label, "Field2");
        assert_eq!(form.active_field, 1);

        let f = form.next_field();
        assert_eq!(f.label, "Field3");
        assert_eq!(form.active_field, 2);

        // wraps back to 0
        let f = form.next_field();
        assert_eq!(f.label, "Field1");
        assert_eq!(form.active_field, 0);
    }

    #[test]
    fn form_prev_field_wraps_from_zero_and_steps_back() {
        let mut form = test_form();
        assert_eq!(form.active_field, 0);

        // prev from 0 wraps to last field without underflowing
        let f = form.prev_field();
        assert_eq!(f.label, "Field3");
        assert_eq!(form.active_field, 2);

        let f = form.prev_field();
        assert_eq!(f.label, "Field2");
        assert_eq!(form.active_field, 1);

        let f = form.prev_field();
        assert_eq!(f.label, "Field1");
        assert_eq!(form.active_field, 0);
    }

    #[test]
    fn form_active_field_mut() {
        let mut form = test_form();
        assert_eq!(form.active_field_mut().unwrap().label, "Field1");
        form.next_field();
        assert_eq!(form.active_field_mut().unwrap().label, "Field2");
    }

    #[test]
    fn new_task_initialization() {
        let fixed_date = NaiveDate::from_ymd_opt(2026, 10, 15)
            .unwrap()
            .and_hms_opt(14, 30, 0)
            .unwrap();
        let form = Form::new_task_with_due(vec![0], "CS101", fixed_date);

        assert_eq!(form.title, "new task · in CS101");
        assert_eq!(form.active_field, 0);
        assert_eq!(form.action, FormAction::CreateTask { parent: vec![0] });
        assert_eq!(form.text_value("name"), Some(""));
        assert_eq!(form.date_value("due"), Some(fixed_date));
    }

    #[test]
    fn new_container_workspace() {
        let form = Form::new_container(vec![], "root", None, ContainerKind::Workspace);

        assert_eq!(form.title, "new Workspace · in root");
        assert_eq!(form.active_field, 0);
        assert_eq!(
            form.action,
            FormAction::CreateContainer {
                parent: vec![],
                kind: ContainerKind::Workspace
            }
        );
        assert_eq!(form.text_value("name"), Some(""));
        assert_eq!(form.text_value("dir"), Some(""));
    }

    #[test]
    fn new_container_project_with_default_dir() {
        let default_dir = PathBuf::from("/home/user/workspace/project");
        let form = Form::new_container(
            vec![1],
            "uni",
            Some(default_dir.clone()),
            ContainerKind::Project,
        );

        assert_eq!(form.title, "new Project · in uni");
        assert_eq!(
            form.action,
            FormAction::CreateContainer {
                parent: vec![1],
                kind: ContainerKind::Project
            }
        );
        assert_eq!(form.text_value("name"), Some(""));
        assert_eq!(form.text_value("dir"), Some(""));
        let FieldInput::Text(dir) = &form.fields[1].input else {
            panic!("dir is not a text field");
        };
        assert_eq!(
            dir.placeholder.as_deref(),
            Some("/home/user/workspace/project/<name>")
        );
    }

    // --------------- Date Field Tests ---------------

    #[test]
    fn form_field_date_wraps_date_input() {
        let f = FormField::date("due", dt(2026, 10, 15, 14, 30));
        assert_eq!(f.label, "due");
        assert_eq!(
            f.input,
            FieldInput::Date(DateInput::new(dt(2026, 10, 15, 14, 30)))
        );
    }

    #[test]
    fn value_getters_only_match_their_own_kind() {
        let form = Form::new_task_with_due(vec![0], "CS101", dt(2026, 10, 15, 14, 30));
        assert_eq!(form.text_value("due"), None); // due is a date
        assert_eq!(form.date_value("name"), None); // name is text
        assert_eq!(form.date_value("missing"), None);
        assert_eq!(form.text_value("missing"), None);
    }

    // --------------- Key Tests ---------------

    use crate::test_util::press;

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
        assert_eq!(form.text_value("Field2"), Some("val2!"));
        assert_eq!(form.text_value("Field1"), Some("val1")); // untouched

        form.handle_key(press(KeyCode::BackTab));
        assert_eq!(form.active_field, 0);
    }

    #[test]
    fn handle_key_reaches_date_fields() {
        let mut form = Form::new_task_with_due(vec![], "root", dt(2026, 6, 15, 12, 0));
        form.handle_key(press(KeyCode::Tab)); // -> due, on Day
        form.handle_key(press(KeyCode::Up));
        assert_eq!(form.date_value("due"), Some(dt(2026, 6, 16, 12, 0)));
    }
}
