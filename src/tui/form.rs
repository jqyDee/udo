use std::path::PathBuf;

use chrono::{DateTime, Utc};

use crate::model::{container::ContainerKind, tree::NodePath};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormField {
    pub label: &'static str,
    pub value: String,
    pub cursor: usize, // character index
    /// Dim hint shown while `value` is empty (e.g. what empty falls back to).
    pub placeholder: Option<String>,
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

impl FormField {
    pub fn new(label: &'static str, value: impl Into<String>) -> Self {
        let value = value.into();
        let cursor = value.chars().count();
        Self {
            label,
            value,
            cursor,
            placeholder: None,
        }
    }

    pub fn with_placeholder(mut self, placeholder: impl Into<String>) -> Self {
        self.placeholder = Some(placeholder.into());
        self
    }

    // --------------- Editing ---------------

    pub fn insert_char(&mut self, c: char) {
        let byte_offset = self.byte_offset_for_char(self.cursor);
        self.value.insert(byte_offset, c);
        self.cursor += 1;
    }

    pub fn backspace(&mut self) {
        // early exit for cursor at position 0
        if self.cursor == 0 {
            return;
        }

        self.cursor -= 1;
        let byte_idx = self.byte_offset_for_char(self.cursor);
        self.value.remove(byte_idx);
    }

    pub fn delete(&mut self) {
        let char_count = self.value.chars().count();

        // early exit for cursor at the end of the input string
        if self.cursor >= char_count {
            return;
        }

        let byte_idx = self.byte_offset_for_char(self.cursor);
        self.value.remove(byte_idx);
    }

    // --------------- Moving ---------------

    pub fn move_left(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }

    pub fn move_right(&mut self) {
        let char_count = self.value.chars().count();
        if self.cursor < char_count {
            self.cursor += 1;
        }
    }

    pub fn move_home(&mut self) {
        self.cursor = 0;
    }

    pub fn move_end(&mut self) {
        self.cursor = self.value.chars().count();
    }

    // --------------- Helpers ---------------

    fn byte_offset_for_char(&self, char_idx: usize) -> usize {
        self.value
            .char_indices()
            .nth(char_idx)
            .map_or(self.value.len(), |(byte_offset, _)| byte_offset)
    }
}

impl Form {
    pub fn new_task(parent: NodePath, parent_name: &str) -> Self {
        Self::new_task_with_due(parent, parent_name, Utc::now())
    }

    pub fn new_task_with_due(parent: NodePath, parent_name: &str, due: DateTime<Utc>) -> Self {
        Self {
            title: format!("new task · in {parent_name}"),
            fields: vec![
                FormField::new("name", ""),
                FormField::new("due", due.format("%Y-%m-%d %H:%M").to_string()),
            ],
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
        let dir = FormField::new("dir", "");
        let dir = match default_dir {
            Some(d) => dir.with_placeholder(format!("{}/<name>", d.display())),
            None => dir,
        };
        Self {
            title: format!("new {:?} · in {parent_name}", kind),
            fields: vec![FormField::new("name", ""), dir],
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

    pub fn field_value(&self, label: &str) -> Option<&str> {
        self.fields
            .iter()
            .find(|f| f.label == label)
            .map(|f| f.value.as_str())
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use chrono::TimeZone;

    use super::*;

    fn form_field_special() -> FormField {
        FormField {
            label: "TestFieldSpecial",
            value: "öüä".to_string(),
            cursor: 1,
            placeholder: None,
        }
    }

    fn test_form() -> Form {
        Form {
            title: "Test Form".into(),
            fields: vec![
                FormField::new("Field1", "val1"),
                FormField::new("Field2", "val2"),
                FormField::new("Field3", "val3"),
            ],
            active_field: 0,
            action: FormAction::CreateTask { parent: vec![] },
        }
    }

    // --------------- Editing Tests ---------------

    #[test]
    fn insert_char() {
        let mut field = FormField::new("Test", "test");
        field.cursor = 0;
        let char_count = field.value.chars().count();
        field.insert_char('b');
        assert_eq!(field.value, "btest");
        assert_eq!(field.cursor, 1);
        assert_eq!(field.value.chars().count(), char_count + 1);
    }

    #[test]
    fn insert_char_at_end() {
        let mut field = FormField::new("Test", "hello");
        assert_eq!(field.cursor, 5);
        field.insert_char('!');
        assert_eq!(field.value, "hello!");
        assert_eq!(field.cursor, 6);
    }

    #[test]
    fn insert_char_special_chars() {
        let mut field = form_field_special();
        let char_count = field.value.chars().count();
        field.insert_char('b');
        assert_eq!(field.value, "öbüä");
        assert_eq!(field.cursor, 2);
        assert_eq!(field.value.chars().count(), char_count + 1);
    }

    #[test]
    fn backspace_from_middle() {
        let mut field = FormField {
            label: "Test",
            value: "test".to_string(),
            cursor: 2,
            placeholder: None,
        };
        field.backspace();
        assert_eq!(field.value, "tst");
        assert_eq!(field.cursor, 1);
    }

    #[test]
    fn backspace_at_start_is_noop() {
        let mut field = FormField {
            label: "Test",
            value: "test".to_string(),
            cursor: 0,
            placeholder: None,
        };
        field.backspace();
        assert_eq!(field.value, "test");
        assert_eq!(field.cursor, 0);
    }

    #[test]
    fn backspace_special_chars() {
        let mut field = form_field_special(); // "öüä", cursor at 1
        field.backspace();
        assert_eq!(field.value, "üä");
        assert_eq!(field.cursor, 0);
    }

    #[test]
    fn delete_from_middle() {
        let mut field = FormField {
            label: "Test",
            value: "test".to_string(),
            cursor: 1,
            placeholder: None,
        };
        field.delete();
        assert_eq!(field.value, "tst");
        assert_eq!(field.cursor, 1);
    }

    #[test]
    fn delete_at_end_is_noop() {
        let mut field = FormField::new("Test", "test");
        field.delete();
        assert_eq!(field.value, "test");
        assert_eq!(field.cursor, 4);
    }

    #[test]
    fn delete_special_chars() {
        let mut field = form_field_special(); // "öüä", cursor at 1
        field.delete();
        assert_eq!(field.value, "öä");
        assert_eq!(field.cursor, 1);
    }

    // --------------- Movement Tests ---------------

    #[test]
    fn move_left() {
        let mut field = FormField {
            label: "Test",
            value: "test".to_string(),
            cursor: 2,
            placeholder: None,
        };
        field.move_left();
        assert_eq!(field.cursor, 1);
    }

    #[test]
    fn move_left_at_zero_stays_zero() {
        let mut field = FormField {
            label: "Test",
            value: "test".to_string(),
            cursor: 0,
            placeholder: None,
        };
        field.move_left();
        assert_eq!(field.cursor, 0);
    }

    #[test]
    fn move_right() {
        let mut field = FormField {
            label: "Test",
            value: "test".to_string(),
            cursor: 1,
            placeholder: None,
        };
        field.move_right();
        assert_eq!(field.cursor, 2);
    }

    #[test]
    fn move_right_stops_at_end() {
        let mut field = FormField::new("Test", "test");
        field.move_right();
        assert_eq!(field.cursor, 4);
    }

    #[test]
    fn move_home() {
        let mut field = FormField::new("Test", "test");
        field.move_home();
        assert_eq!(field.cursor, 0);
    }

    #[test]
    fn move_end() {
        let mut field = FormField {
            label: "Test",
            value: "test".to_string(),
            cursor: 1,
            placeholder: None,
        };
        field.move_end();
        assert_eq!(field.cursor, 4);
    }

    #[test]
    fn move_end_empty_string() {
        let mut field = FormField::new("Test", "");
        field.move_end();
        assert_eq!(field.cursor, 0);
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
        let fixed_date = Utc.with_ymd_and_hms(2026, 10, 15, 14, 30, 0).unwrap();
        let form = Form::new_task_with_due(vec![0], "CS101", fixed_date);

        assert_eq!(form.title, "new task · in CS101");
        assert_eq!(form.active_field, 0);
        assert_eq!(form.action, FormAction::CreateTask { parent: vec![0] });
        assert_eq!(form.field_value("name"), Some(""));
        assert_eq!(form.field_value("due"), Some("2026-10-15 14:30"));
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
        assert_eq!(form.field_value("name"), Some(""));
        assert_eq!(form.field_value("dir"), Some(""));
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
        assert_eq!(form.field_value("name"), Some(""));
        assert_eq!(form.field_value("dir"), Some(""));
        assert_eq!(
            form.fields[1].placeholder.as_deref(),
            Some("/home/user/workspace/project/<name>")
        );
    }
}
