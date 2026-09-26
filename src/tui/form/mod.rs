//! Forms for creating and editing nodes: a list of fields plus what to do
//! on submit.
//!
//! Both are filled the same way (`Form::for_node`): editing starts from the
//! node, creating from a template node with the defaults. Both are read the
//! same way (`Form::values`).
//!
//! - `text`: `TextInput`, free text with a cursor
//! - `date`: `DateInput`, local date + time edited by segment
//! - `choice`: `ChoiceInput`, one of a few options (←/→)

mod choice;
mod date;
mod text;

use std::path::PathBuf;

use chrono::{Local, NaiveDateTime, Utc};
use crossterm::event::{KeyCode, KeyEvent};

pub use choice::{
    CONTAINER_FOLDER_CHOICES, CONTAINER_KIND_CHOICES, ChoiceInput, FOLDER_CHOICES, FolderMode,
    kind_from_label,
};
pub use date::{DateInput, Segment, local_to_utc};
pub use text::TextInput;

use crate::{
    dir::parse_abs_dir,
    model::{
        NodePath,
        container::{Container, ContainerKind},
        node::{Node, NodeBody},
        task::Task,
    },
    naming::{folder_name, normalize_name},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormField {
    pub id: FieldId,
    pub input: FieldInput,
}

/// Which value a field holds. Lookups go by id, not by label string, so a
/// typo is a compile error instead of a silent `None`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldId {
    Name,
    Due,
    Dir,
    Folder,
    Kind,
    Description,
}

impl FieldId {
    /// Shown in front of the value.
    pub fn label(self) -> &'static str {
        match self {
            Self::Name => "name",
            Self::Due => "due",
            Self::Dir => "dir",
            Self::Folder => "folder",
            Self::Kind => "kind",
            Self::Description => "description",
        }
    }
}

/// What kind of value a field holds, and so which keys edit it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FieldInput {
    Text(TextInput),
    Date(DateInput),
    Choice(ChoiceInput),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FormAction {
    CreateTask {
        parent: NodePath,
    },
    /// Kind comes from the `kind` field (`Form::container_kind`).
    CreateContainer {
        parent: NodePath,
    },
    EditNode {
        path: NodePath,
    },
}

pub struct TaskDefaults {
    pub due: NaiveDateTime,
    pub folder: FolderMode,
}

/// A filled-in form, independent of create or edit (`Form::values`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormValues {
    /// Normalized like every name (`normalize_name`).
    pub name: String,
    /// As typed; the tree trims it and turns blank into "no description".
    pub description: String,
    /// Task forms only; local time, as typed.
    pub due: Option<NaiveDateTime>,
    /// Container forms only.
    pub kind: Option<ContainerKind>,
}

/// Folder choice + dir row (create forms only).
fn folder_rows(choices: &'static [&'static str], mode: FolderMode) -> [FormField; 2] {
    [
        FormField {
            id: FieldId::Folder,
            input: FieldInput::Choice(ChoiceInput::new(choices, mode.label())),
        },
        FormField {
            id: FieldId::Dir,
            input: FieldInput::Text(TextInput::new("").with_placeholder("/… or ~/…")),
        },
    ]
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Form {
    pub title: String,
    pub fields: Vec<FormField>,
    pub parent_dir: Option<PathBuf>,
    pub active_field: usize,
    pub action: FormAction,
}

/// What the app should do after a key went into the form.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormOutcome {
    /// Keep editing.
    Continue,
    /// Enter: validate and save (`App::submit_form`).
    Submit,
    /// Esc: close without saving.
    Cancel,
}

impl FormField {
    /// Text field, cursor at the end of `value`.
    pub fn text(id: FieldId, value: impl Into<String>) -> Self {
        Self {
            id,
            input: FieldInput::Text(TextInput::new(value)),
        }
    }

    /// Date field, starting on `Segment::Day` (the part changed most).
    pub fn date(id: FieldId, value: NaiveDateTime) -> Self {
        Self {
            id,
            input: FieldInput::Date(DateInput::new(value)),
        }
    }
}

impl Form {
    /// Create form for a task: a template node with the due date from
    /// `defaults`. Folder rows start in `defaults.folder`; the dir row is only
    /// edited in `custom` mode, `auto` / `none` show a preview instead (see
    /// `dir_preview`).
    pub fn new_task(
        parent: NodePath,
        parent_name: &str,
        parent_dir: Option<PathBuf>,
        defaults: TaskDefaults,
    ) -> Self {
        // template stores UTC like a real task; `for_node` shows it local again
        let due = local_to_utc(defaults.due).unwrap_or_else(Utc::now);
        let template = Node::task(String::new(), Task::new(None, due));
        Self::for_node(
            format!("new task · in {parent_name}"),
            FormAction::CreateTask { parent },
            &template,
            parent_dir,
            Some(defaults.folder),
        )
    }

    /// Create form for a container: a template node of the guessed `kind`
    /// (changeable in the form, nested workspaces are fine). Folder: `auto`
    /// or `custom`, never `none`.
    pub fn new_container(
        parent: NodePath,
        parent_name: &str,
        parent_dir: Option<PathBuf>,
        kind: ContainerKind,
    ) -> Self {
        // the dir comes from the folder rows on submit, this one is never used
        let template = Node::container(String::new(), Container::new(PathBuf::new(), kind));
        Self::for_node(
            format!("new container · in {parent_name}"),
            FormAction::CreateContainer { parent },
            &template,
            parent_dir,
            Some(FolderMode::Auto),
        )
    }

    /// Edit form for an existing node, prefilled with its values. No folder
    /// rows: dirs can't be changed yet.
    pub fn edit_node(path: NodePath, node: &Node) -> Self {
        let kind = match node.body {
            NodeBody::Container(_) => "container",
            NodeBody::Task(_) => "task",
        };
        Self::for_node(
            format!("edit {kind} · {}", node.name()),
            FormAction::EditNode { path },
            node,
            None,
            None,
        )
    }

    /// Fields for `node`, prefilled with its values. Same fields in the same
    /// order for creating and editing: task = name, description, [folder,
    /// dir], due; container = name, description, kind, [folder, dir].
    /// `folder`: `Some(mode)` adds the folder + dir rows (create), `None`
    /// leaves them out (edit).
    fn for_node(
        title: String,
        action: FormAction,
        node: &Node,
        parent_dir: Option<PathBuf>,
        folder: Option<FolderMode>,
    ) -> Self {
        let description = node.header.description.clone().unwrap_or_default();
        let mut fields = vec![
            FormField::text(FieldId::Name, node.name()),
            FormField {
                id: FieldId::Description,
                input: FieldInput::Text(TextInput::new(description).with_placeholder("optional")),
            },
        ];
        match &node.body {
            NodeBody::Task(t) => {
                if let Some(mode) = folder {
                    fields.extend(folder_rows(FOLDER_CHOICES, mode));
                }
                // stored as UTC, edited as local time
                let due = t.due_date.with_timezone(&Local).naive_local();
                fields.push(FormField::date(FieldId::Due, due));
            }
            NodeBody::Container(c) => {
                fields.push(FormField {
                    id: FieldId::Kind,
                    input: FieldInput::Choice(ChoiceInput::new(
                        CONTAINER_KIND_CHOICES,
                        c.kind.label(),
                    )),
                });
                if let Some(mode) = folder {
                    fields.extend(folder_rows(CONTAINER_FOLDER_CHOICES, mode));
                }
            }
        }
        Self {
            title,
            fields,
            parent_dir,
            active_field: 0,
            action,
        }
    }

    /// What the form says right now, for create and edit alike. Raw input:
    /// the tree checks names and cleans descriptions.
    pub fn values(&self) -> FormValues {
        FormValues {
            name: normalize_name(self.text_value(FieldId::Name).unwrap_or("")),
            description: self
                .text_value(FieldId::Description)
                .unwrap_or("")
                .to_string(),
            due: self.date_value(FieldId::Due),
            kind: self.container_kind(),
        }
    }

    pub fn active_field_mut(&mut self) -> Option<&mut FormField> {
        self.fields.get_mut(self.active_field)
    }

    /// Next field Tab can land on (see `is_focusable`), wrapping around.
    pub fn next_field(&mut self) -> &mut FormField {
        let field_count = self.fields.len();
        assert!(field_count > 0, "Form has no fields");
        // at most one round, so a form without focusable fields can't hang
        for _ in 0..field_count {
            self.active_field = (self.active_field + 1) % field_count;
            if self.is_focusable(self.active_field) {
                break;
            }
        }
        &mut self.fields[self.active_field]
    }

    /// Previous field Tab can land on (see `is_focusable`), wrapping around.
    pub fn prev_field(&mut self) -> &mut FormField {
        let field_count = self.fields.len();
        assert!(field_count > 0, "Form has no fields");
        for _ in 0..field_count {
            self.active_field = (self.active_field + field_count - 1) % field_count;
            if self.is_focusable(self.active_field) {
                break;
            }
        }
        &mut self.fields[self.active_field]
    }

    /// Whether Tab can land on field `i`: every field, except the dir row
    /// outside `custom` mode (it only shows a preview there).
    fn is_focusable(&self, i: usize) -> bool {
        match self.fields.get(i) {
            Some(f) if f.id == FieldId::Dir => {
                self.folder_mode().is_none_or(|m| m == FolderMode::Custom)
            }
            Some(_) => true,
            None => false,
        }
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
            _ => {
                let mode_before = self.folder_mode();
                match self.active_field_mut().map(|f| &mut f.input) {
                    Some(FieldInput::Text(t)) => t.handle_key(key),
                    Some(FieldInput::Date(d)) => d.handle_key(key),
                    Some(FieldInput::Choice(c)) => c.handle_key(key),
                    None => {}
                }
                if self.folder_mode() != mode_before {
                    self.on_folder_changed();
                }
            }
        }
        FormOutcome::Continue
    }

    /// Switched to `custom`: start the dir text from the auto path (or the
    /// parent dir while the name is empty). Typed text is kept, so switching
    /// back and forth loses nothing.
    fn on_folder_changed(&mut self) {
        if self.folder_mode() != Some(FolderMode::Custom) {
            return;
        }
        let start = match (self.auto_dir(), &self.parent_dir) {
            (Some(auto), _) => auto.display().to_string(),
            (None, Some(parent)) => format!("{}/", parent.display()),
            (None, None) => return,
        };
        let dir = self.fields.iter_mut().find(|f| f.id == FieldId::Dir);
        if let Some(FieldInput::Text(t)) = dir.map(|f| &mut f.input)
            && t.value.is_empty()
        {
            t.cursor = start.chars().count();
            t.value = start;
        }
    }

    /// Value of the text field `id`. None if missing or not a text field.
    pub fn text_value(&self, id: FieldId) -> Option<&str> {
        match &self.fields.iter().find(|f| f.id == id)?.input {
            FieldInput::Text(t) => Some(t.value.as_str()),
            _ => None,
        }
    }

    /// Value of the date field `id`. None if missing or not a date field.
    pub fn date_value(&self, id: FieldId) -> Option<NaiveDateTime> {
        match &self.fields.iter().find(|f| f.id == id)?.input {
            FieldInput::Date(d) => Some(d.value),
            _ => None,
        }
    }

    /// Selected index of the choice field `id`. None if missing or not a
    /// choice field.
    pub fn choice_value(&self, id: FieldId) -> Option<usize> {
        match &self.fields.iter().find(|f| f.id == id)?.input {
            FieldInput::Choice(c) => Some(c.selected),
            _ => None,
        }
    }

    /// Label of the chosen option of the choice field `id`. None if missing
    /// or not a choice field.
    pub fn choice_label(&self, id: FieldId) -> Option<&'static str> {
        match &self.fields.iter().find(|f| f.id == id)?.input {
            FieldInput::Choice(c) => c.selected_label(),
            _ => None,
        }
    }

    /// Current folder mode, if the form has a folder field. By label, since
    /// task and container forms offer different options.
    pub fn folder_mode(&self) -> Option<FolderMode> {
        FolderMode::from_label(self.choice_label(FieldId::Folder)?)
    }

    /// Chosen kind, if the form has a kind field (container forms).
    pub fn container_kind(&self) -> Option<ContainerKind> {
        kind_from_label(self.choice_label(FieldId::Kind)?)
    }

    /// The folder to create on submit: `auto` -> `auto_dir`, `custom` -> the
    /// typed text checked by `parse_abs_dir` (Err: message for a toast),
    /// `none` or no folder field -> None.
    pub fn chosen_dir(&self) -> Result<Option<PathBuf>, String> {
        match self.folder_mode() {
            Some(FolderMode::Auto) => Ok(self.auto_dir()),
            Some(FolderMode::Custom) => {
                parse_abs_dir(self.text_value(FieldId::Dir).unwrap_or("")).map(Some)
            }
            Some(FolderMode::None) | None => Ok(None),
        }
    }

    /// Where `auto` puts the folder: `<parent dir>/<folder name>`. None
    /// without a parent dir or while the name is empty.
    pub fn auto_dir(&self) -> Option<PathBuf> {
        let name = self.text_value(FieldId::Name)?;
        Some(self.parent_dir.as_ref()?.join(folder_name(name)?))
    }

    /// The folder the node would get right now: `auto` -> `auto_dir`,
    /// `custom` -> the typed text (not validated yet), `none` -> None. None
    /// too for forms without a folder field.
    pub fn dir_preview(&self) -> Option<PathBuf> {
        match self.folder_mode()? {
            FolderMode::Auto => self.auto_dir(),
            FolderMode::Custom => {
                let text = self.text_value(FieldId::Dir)?.trim();
                (!text.is_empty()).then(|| PathBuf::from(text))
            }
            FolderMode::None => None,
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
        let task = Task::new(Some("/uni/lab".into()), local_to_utc(due).unwrap());
        let node = Node::task("lab 3".into(), task).with_description(Some("ex 1-4".into()));

        let form = Form::edit_node(vec![0, 1], &node);

        assert_eq!(form.title, "edit task · lab 3");
        assert_eq!(form.action, FormAction::EditNode { path: vec![0, 1] });
        assert_eq!(ids(&form), [FieldId::Name, FieldId::Description, FieldId::Due]);
        let v = form.values();
        assert_eq!(v.name, "lab 3");
        assert_eq!(v.description, "ex 1-4");
        assert_eq!(v.due, Some(due)); // UTC -> local round trip
        assert_eq!(v.kind, None);
        assert_eq!(form.chosen_dir(), Ok(None)); // no folder rows: dir untouched
    }

    #[test]
    fn edit_container_is_prefilled_with_its_kind() {
        let c = Container::new("/uni".into(), ContainerKind::Project);
        let form = Form::edit_node(vec![0], &Node::container("uni".into(), c));

        assert_eq!(form.title, "edit container · uni");
        assert_eq!(ids(&form), [FieldId::Name, FieldId::Description, FieldId::Kind]);
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
        let edit = Form::edit_node(vec![0], &Node::task("t".into(), Task::new(None, Utc::now())));
        // edit = create without the folder rows
        let without_folder: Vec<_> = ids(&create)
            .into_iter()
            .filter(|id| !matches!(id, FieldId::Folder | FieldId::Dir))
            .collect();
        assert_eq!(ids(&edit), without_folder);
    }

    #[test]
    fn values_normalize_the_name_but_keep_the_description_raw() {
        let mut form = Form::edit_node(vec![0], &Node::task("a".into(), Task::new(None, Utc::now())));
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
        let Some(FormField { input: FieldInput::Text(t), .. }) = dir else {
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
}
