//! Forms for creating and editing nodes: a list of fields plus what to do
//! on submit.
//!
//! Both are filled the same way (`Form::for_node`): editing starts from the
//! node, creating from a template node with the defaults. Each kind of form
//! has one typed reader that checks the input and converts local dates to
//! `Time` (`new_task_node`, `new_container_node`, `node_edit`, `settings`,
//! `session_times`, `split_at`, `cut_range`, `add_times`). Settings have
//! their own form (`Form::edit_settings`), sessions too (`edit_session`,
//! `split_session`, `cut_session`, `add_session`).
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

use chrono::{Local, NaiveDateTime, TimeDelta, Timelike};
use crossterm::event::{KeyCode, KeyEvent};

pub use choice::{
    CONTAINER_FOLDER_CHOICES, CONTAINER_KIND_CHOICES, ChoiceInput, FOLDER_CHOICES, FolderMode,
    kind_from_label,
};
pub use date::{DateInput, Segment};
pub use text::TextInput;

use crate::{
    dir::{default_dir, parse_abs_dir},
    model::{
        NodePath,
        container::{Container, ContainerKind, ContainerPatch},
        node::{BodyPatch, HeaderPatch, Node, NodeBody, NodePatch},
        sessions::{Session, SessionId},
        settings::{
            ContainerSettings, RootSettings,
            view::{ROOT_SETTINGS, SETTINGS, SettingInfo},
        },
        task::{Task, TaskPatch},
        time::{Time, local_to_fixed, now},
    },
    naming::normalize_name,
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
    Start,
    End,
    /// Split: where.
    At,
    /// Cut: the part to remove, `[From, To)`.
    From,
    To,
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
            Self::Start => "start",
            Self::End => "end",
            Self::At => "at",
            Self::From => "from",
            Self::To => "to",
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

impl FieldInput {
    pub fn handle_key(&mut self, key: KeyEvent) -> bool {
        match self {
            Self::Text(t) => t.handle_key(key),
            Self::Date(d) => d.handle_key(key),
            Self::Choice(c) => c.handle_key(key),
        }
    }
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
    /// Start / end of a session (`Form::session_times`).
    EditSession {
        id: SessionId,
    },
    /// One session becomes two at `Form::split_at`.
    SplitSession {
        id: SessionId,
    },
    /// `Form::cut_range` is removed from a session.
    CutSession {
        id: SessionId,
    },
    /// A new manual session on the task at `path` (`Form::add_times`).
    AddSession {
        path: NodePath,
    },
}

/// Minute step of the session form's dates: corrections are often a few
/// minutes (due dates keep `DEFAULT_MINUTE_STEP`).
pub const SESSION_MINUTE_STEP: i64 = 1;

/// How much a new cut form removes, from the midpoint on (a lunch break).
const CUT_DEFAULT: TimeDelta = TimeDelta::minutes(30);

/// How long a new add form's session is: the last hour, up to now.
const ADD_DEFAULT: TimeDelta = TimeDelta::hours(1);

pub struct TaskDefaults {
    pub due: NaiveDateTime,
    pub folder: FolderMode,
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
    /// Enter: validate and save (`App::save`).
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
        // template stores a `Time` like a real task; `for_node` shows it local again
        let due = local_to_fixed(defaults.due).unwrap_or_else(now);
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

    /// Add form for the task `task_name` at `path`: the last hour, `end` on
    /// now rounded down to the minute, `start` `ADD_DEFAULT` before. It may
    /// overlap another session (timer running, a session in the last
    /// hour): saving refuses that, the form stays.
    pub fn add_session(task_name: &str, path: NodePath, now: Time) -> Self {
        let end = local_minute(now);
        Self {
            title: format!("add session · {task_name}"),
            fields: vec![
                session_date(FieldId::Start, end - ADD_DEFAULT),
                session_date(FieldId::End, end),
            ],
            parent_dir: None,
            active_field: 0,
            action: FormAction::AddSession { path },
        }
    }

    /// Edit form of a session: start and end as dates (local, to the
    /// minute; ↑/↓ on the minutes by `SESSION_MINUTE_STEP`). A running
    /// session has only a start (stop the timer to end it).
    pub fn edit_session(session: &Session) -> Self {
        let mut fields = vec![session_date(FieldId::Start, local(session.start))];
        if let Some(end) = session.end {
            fields.push(session_date(FieldId::End, local(end)));
        }
        Self {
            title: session_title("edit", session),
            fields,
            parent_dir: None,
            active_field: 0,
            action: FormAction::EditSession { id: session.id },
        }
    }

    /// Split form: `at` starts on the session's midpoint (running: up to
    /// `now`), rounded down to the minute. A running session can't be
    /// split: the app says so before opening, the store refuses it on save.
    pub fn split_session(session: &Session, now: Time) -> Self {
        let mid = midpoint(session, now);
        Self {
            title: session_title("split", session),
            fields: vec![session_date(FieldId::At, local_minute(mid))],
            parent_dir: None,
            active_field: 0,
            action: FormAction::SplitSession { id: session.id },
        }
    }

    /// Cut form: `from` on the midpoint, `to` `CUT_DEFAULT` later, capped
    /// at the end (running: at `now`); both rounded down to the minute.
    pub fn cut_session(session: &Session, now: Time) -> Self {
        let mid = midpoint(session, now);
        let to = (mid + CUT_DEFAULT).min(session.end.unwrap_or(now));
        Self {
            title: session_title("cut", session),
            fields: vec![
                session_date(FieldId::From, local_minute(mid)),
                session_date(FieldId::To, local_minute(to)),
            ],
            parent_dir: None,
            active_field: 0,
            action: FormAction::CutSession { id: session.id },
        }
    }

    /// Where a split form splits. Err: DST gap. On the start / end, outside
    /// the session, running: checked by `Core` / the store.
    pub fn split_at(&self) -> Result<Time, String> {
        to_time(
            self.date_value(FieldId::At)
                .expect("split forms always have `at`"),
        )
    }

    /// What a cut form removes, `[from, to)`. Err: DST gap. Order, overlap
    /// with the session and the future: checked by `Core` / the store.
    pub fn cut_range(&self) -> Result<(Time, Time), String> {
        let from = self
            .date_value(FieldId::From)
            .expect("cut forms always have `from`");
        let to = self
            .date_value(FieldId::To)
            .expect("cut forms always have `to`");
        Ok((to_time(from)?, to_time(to)?))
    }

    /// Start and end of an add form. Err: DST gap. Order, overlap and the
    /// future: checked by `Core` / the store.
    pub fn add_times(&self) -> Result<(Time, Time), String> {
        let start = self
            .date_value(FieldId::Start)
            .expect("add forms always have a start");
        let end = self
            .date_value(FieldId::End)
            .expect("add forms always have an end");
        Ok((to_time(start)?, to_time(end)?))
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
                // stored with its offset, edited as current local time
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

    /// Checked input of a task create form, as the task to create. Name
    /// rules are checked by the tree. Err: DST gap in the due date, custom
    /// dir not absolute.
    pub fn new_task_node(&self) -> Result<Node, String> {
        let due = self.due()?.expect("task forms have a due field");
        // by folder mode; clashes are checked by the tree
        let dir = self.chosen_dir()?;
        Ok(Node::task(self.name(), Task::new(dir, due)).with_description(Some(self.description())))
    }

    /// Checked input of a container create form, as the container to
    /// create. Err: `auto` without a name, custom dir not absolute.
    pub fn new_container_node(&self) -> Result<Node, String> {
        let kind = self
            .container_kind()
            .expect("container forms have a kind field");
        // auto without a name: say what's missing
        let dir = self.chosen_dir()?.ok_or("name cannot be empty")?;
        Ok(Node::container(self.name(), Container::new(dir, kind))
            .with_description(Some(self.description())))
    }

    /// Checked input of an edit form, as a patch. Always every field: an
    /// unchanged name passes the checks, an emptied description is removed
    /// by the tree. Err: DST gap in the due date.
    pub fn node_edit(&self) -> Result<NodePatch, String> {
        // only task forms have a due date, only container forms a kind
        let body = match (self.due()?, self.container_kind()) {
            (Some(due), _) => Some(BodyPatch::Task(TaskPatch {
                due_date: Some(due),
                ..Default::default()
            })),
            (_, Some(kind)) => Some(BodyPatch::Container(ContainerPatch {
                kind: Some(kind),
                ..Default::default()
            })),
            (None, None) => None,
        };
        Ok(NodePatch {
            header: HeaderPatch {
                name: Some(self.name()),
                description: Some(Some(self.description())),
            },
            body,
        })
    }

    /// Start and end of a session form as `Time` (end: None for a running
    /// session). Err: DST gap. Other checks (future, overlap) are the
    /// store's.
    pub fn session_times(&self) -> Result<(Time, Option<Time>), String> {
        let start = self
            .date_value(FieldId::Start)
            .expect("session forms always have a start");
        let end = self.date_value(FieldId::End).map(to_time).transpose()?;
        Ok((to_time(start)?, end))
    }

    /// The name, normalized like every name (`normalize_name`).
    pub fn name(&self) -> String {
        normalize_name(self.text_value(FieldId::Name).unwrap_or(""))
    }

    /// As typed; the tree trims it and turns blank into "no description".
    fn description(&self) -> String {
        self.text_value(FieldId::Description)
            .unwrap_or("")
            .to_string()
    }

    /// The due date as a `Time` (task forms only, else None). Err: DST gap.
    fn due(&self) -> Result<Option<Time>, String> {
        self.date_value(FieldId::Due).map(to_time).transpose()
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
                if let Some(field) = self.active_field_mut() {
                    let id = field.id;
                    if field.input.handle_key(key) {
                        self.on_change(id);
                    }
                }
            }
        }
        FormOutcome::Continue
    }

    fn on_change(&mut self, id: FieldId) {
        if id == FieldId::Folder {
            self.on_folder_changed();
        }
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
        default_dir(self.parent_dir.as_ref()?, name)
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

/// A date from a form (local, as shown) as a `Time`. Err: a local time a
/// DST switch skips, for the toast.
fn to_time(local: NaiveDateTime) -> Result<Time, String> {
    local_to_fixed(local).ok_or_else(|| "that time doesn't exist (DST switch)".into())
}

/// A session's date field: local, ↑/↓ on the minutes by
/// `SESSION_MINUTE_STEP`.
fn session_date(id: FieldId, local: NaiveDateTime) -> FormField {
    FormField {
        id,
        input: FieldInput::Date(DateInput::new(local).with_minute_step(SESSION_MINUTE_STEP)),
    }
}

/// `t` local, as shown, seconds kept: an unchanged edit form must read
/// back exactly the session's times (`save_session` compares them).
fn local(t: Time) -> NaiveDateTime {
    t.with_timezone(&Local).naive_local()
}

/// `t` local, rounded down to the minute: where the defaults of the split
/// and cut forms start.
fn local_minute(t: Time) -> NaiveDateTime {
    local(t)
        .with_second(0)
        .and_then(|l| l.with_nanosecond(0))
        .expect("0 seconds always exists")
}

/// Halfway between the session's start and end (running: `now`).
fn midpoint(session: &Session, now: Time) -> Time {
    let end = session.end.unwrap_or(now);
    session.start + (end - session.start) / 2
}

/// `split session · lab 3`; running: `… (running)`.
fn session_title(verb: &str, session: &Session) -> String {
    let running = if session.end.is_none() {
        " (running)"
    } else {
        ""
    };
    format!("{verb} session · {}{running}", session.task.name)
}
