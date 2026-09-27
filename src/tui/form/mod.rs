//! Forms for creating and editing nodes: a list of fields plus what to do
//! on submit.
//!
//! Both are filled the same way (`Form::for_node`): editing starts from the
//! node, creating from a template node with the defaults. Both are read the
//! same way (`Form::values`). Settings have their own form
//! (`Form::edit_settings`, read with `Form::settings`).
//!
//! - `text`: `TextInput`, free text with a cursor
//! - `date`: `DateInput`, local date + time edited by segment
//! - `choice`: `ChoiceInput`, one of a few options (←/→)

mod choice;
mod date;
#[cfg(test)]
mod tests;
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
        settings::{
            ContainerSettings, RootSettings,
            view::{ROOT_SETTINGS, SETTINGS, SettingInfo},
        },
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
    /// Index into `SETTINGS`.
    Setting(usize),
    /// Index into `ROOT_SETTINGS`.
    RootSetting(usize),
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
            Self::Setting(idx) => SETTINGS[idx].label,
            Self::RootSetting(idx) => ROOT_SETTINGS[idx].label,
        }
    }

    /// How a setting's values are written (`SettingInfo::format`), for the
    /// help below the form. None for every other field.
    pub fn format_hint(self) -> Option<&'static str> {
        match self {
            Self::Setting(idx) => SETTINGS[idx].format,
            Self::RootSetting(idx) => ROOT_SETTINGS[idx].format,
            _ => None,
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
    /// Own settings of the container at `path` (`Form::settings`).
    EditSettings {
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

/// Input for one setting: a choice if it has fixed `choices` (the unset
/// option first, labelled `unset_label`), else free text. `value`: set in
/// this file (None = unset). `hint`: what applies when unset (the
/// placeholder of a text field, shown next to a choice).
fn setting_input(
    choices: &'static [&'static str],
    value: Option<String>,
    unset_label: &'static str,
    hint: Option<String>,
) -> FieldInput {
    if choices.is_empty() {
        let placeholder = hint.unwrap_or_else(|| unset_label.into());
        return FieldInput::Text(
            TextInput::new(value.unwrap_or_default()).with_placeholder(placeholder),
        );
    }
    let mut c = ChoiceInput::unsettable(unset_label, choices, value.as_deref());
    c.hint = hint;
    FieldInput::Choice(c)
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

    /// Settings form for the container at `path` (called `name`): one text
    /// field per `SETTINGS` entry with the own value (empty = inherit), the
    /// placeholder from `inherited` (what applies when it's empty). `root`:
    /// the root's `[root]` settings, one more field per `ROOT_SETTINGS`
    /// entry; None for every other container.
    pub fn edit_settings(
        path: NodePath,
        name: &str,
        own: &ContainerSettings,
        inherited: impl Fn(&SettingInfo<ContainerSettings>) -> String,
        root: Option<&RootSettings>,
    ) -> Self {
        let mut fields: Vec<_> = SETTINGS
            .iter()
            .enumerate()
            .map(|(i, info)| FormField {
                id: FieldId::Setting(i),
                input: setting_input(
                    info.choices,
                    (info.get)(own),
                    "inherit",
                    Some(inherited(info)),
                ),
            })
            .collect();
        if let Some(root) = root {
            // not inherited: nothing applies when unset
            fields.extend(ROOT_SETTINGS.iter().enumerate().map(|(i, info)| FormField {
                id: FieldId::RootSetting(i),
                input: setting_input(info.choices, (info.get)(root), "not set", None),
            }));
        }
        Self {
            title: format!("settings · {name}"),
            fields,
            parent_dir: None,
            active_field: 0,
            action: FormAction::EditSettings { path },
        }
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

    /// What a settings form says right now, parsed by the `set` of each
    /// entry, for `Tree::set_settings`. Root settings: Some only if the form
    /// has their fields (root). Err: `"deadline: <why>"`, for a toast.
    pub fn settings(&self) -> Result<(ContainerSettings, Option<RootSettings>), String> {
        let mut settings = ContainerSettings::default();
        let mut root: Option<RootSettings> = None;
        for field in &self.fields {
            // as text for `set`: the unset option of a choice is blank
            let text = match &field.input {
                FieldInput::Text(t) => t.value.as_str(),
                FieldInput::Choice(c) => c.value().unwrap_or(""),
                FieldInput::Date(_) => continue,
            };
            match field.id {
                FieldId::Setting(i) => {
                    let info = &SETTINGS[i];
                    (info.set)(&mut settings, text).map_err(|e| format!("{}: {e}", info.label))?;
                }
                FieldId::RootSetting(i) => {
                    let info = &ROOT_SETTINGS[i];
                    (info.set)(root.get_or_insert_default(), text)
                        .map_err(|e| format!("{}: {e}", info.label))?;
                }
                _ => {}
            }
        }
        Ok((settings, root))
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
