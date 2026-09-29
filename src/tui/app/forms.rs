//! Forms: `t` / `T` new task, `c` / `C` new container, `e` edit (on the
//! settings tab: the container's settings; in the sessions list: the
//! session, saved in `sessions`). Opening
//! picks the parent (or the node), the form edits itself
//! (`Form::handle_key`), submit calls the tree (which checks names and
//! creates dirs).

use chrono::{Local, NaiveDateTime};
use crossterm::event::KeyEvent;

use super::{App, Flow, Mode};
use crate::{
    model::{
        NodePath,
        container::{Container, ContainerKind, ContainerPatch},
        node::{BodyPatch, HeaderPatch, Node, NodePatch},
        settings::{ContainerSettings, view::SettingInfo},
        task::{Task, TaskPatch},
        time::{Time, local_to_fixed},
        tree::Tree,
    },
    tui::form::{FolderMode, Form, FormAction, FormOutcome, TaskDefaults},
};

impl App<'_> {
    /// Where a new node goes: the root if `global`, else the container at
    /// the cursor (or the task's container).
    fn creation_parent(&self) -> NodePath {
        self.core
            .tree()
            .nearest_file_owner(&self.tree_state.cursor)
            .unwrap_or_default()
    }

    pub(super) fn open_container_form(&mut self) {
        let parent = self.creation_parent();
        let parent_node = self.core.tree().get(&parent);
        let parent_name = parent_node.map_or("root", |n| n.name());
        let parent_dir = parent_node.and_then(|n| n.dir().map(|d| d.to_path_buf()));

        let kind = if parent.is_empty() {
            ContainerKind::Workspace
        } else {
            ContainerKind::Project
        };

        self.mode =
            Mode::Form(Box::new(Form::new_container(parent, parent_name, parent_dir, kind)));
    }

    pub(super) fn open_task_form(&mut self) {
        let parent = self.creation_parent();
        let parent_node = self.core.tree().get(&parent);
        let parent_name = parent_node.map_or("root", |n| n.name());
        let parent_dir = parent_node.and_then(|n| n.dir().map(|d| d.to_path_buf()));

        let d = self.core.task_defaults(&parent, Local::now().naive_local());
        let defaults = TaskDefaults {
            due: d.due,
            folder: FolderMode::from(d.task_folders),
        };

        self.mode = Mode::Form(Box::new(Form::new_task(parent, parent_name, parent_dir, defaults)));
    }

    /// Edit form for the node at the cursor, prefilled with its values.
    pub(super) fn open_edit_form(&mut self) {
        if self.tree_state.on_root() {
            return self.error("the root cannot be edited");
        }
        match self.tree_state.selected(self.core.tree()) {
            Some(node) => {
                let path = self.tree_state.cursor.clone();
                self.mode = Mode::Form(Box::new(Form::edit_node(path, node)));
            }
            None => self.error("nothing selected"),
        }
    }

    /// Settings form for the container at the cursor (a task: its
    /// container). Placeholders show what applies when a field is empty;
    /// the root row also gets the `[root]` settings.
    pub(super) fn open_settings_form(&mut self) {
        let tree: &Tree = self.core.tree();
        let Some(path) = tree.nearest_file_owner(&self.tree_state.cursor) else {
            return self.error("nothing selected");
        };
        let Some((node, c)) = tree.get(&path).and_then(|n| Some((n, n.as_container()?))) else {
            return self.error("nothing selected");
        };
        let inherited =
            |info: &SettingInfo<ContainerSettings>| match tree.inherited_setting(&path, info.get) {
                Some(r) => format!("{} ({})", r.value, tree.source_text(&r.source)),
                None => "not set".into(),
            };
        let root = path.is_empty().then_some(&c.root_settings);
        let form = Form::edit_settings(path.clone(), node.name(), &c.settings, inherited, root);
        self.mode = Mode::Form(Box::new(form));
    }

    pub(super) async fn handle_form_key(&mut self, key: KeyEvent) -> Flow {
        let Mode::Form(form) = &mut self.mode else {
            return Flow::Continue;
        };
        match form.handle_key(key) {
            FormOutcome::Continue => {}
            FormOutcome::Cancel => self.mode = mode_after(&form.action),
            FormOutcome::Submit => self.submit_form().await,
        }
        Flow::Continue
    }

    /// Create or edit the node. The input is read and checked once, then each
    /// action only builds a node (create) or a patch (edit) for the tree.
    /// Success: close the form, select the node. Error: toast, form stays
    /// open so the input can be fixed. A session form has its own path
    /// (`submit_session_form`): none of the node rules apply.
    async fn submit_form(&mut self) {
        let Mode::Form(form) = &self.mode.clone() else {
            return;
        };
        if let FormAction::EditSession { id } = form.action {
            return self.submit_session_form(id, form).await;
        }

        // name rules (empty, `/`, `..`, duplicates) are checked by the tree
        let v = form.values();
        let description = Some(v.description);
        // only task forms have a due date
        let due = match v.due.map(to_time).transpose() {
            Ok(due) => due,
            Err(e) => return self.error(e),
        };
        // by folder mode (create forms only); clashes are checked by the tree
        let dir = match form.chosen_dir() {
            Ok(dir) => dir,
            Err(e) => {
                self.error(e);
                return;
            }
        };

        let saved = match &form.action {
            FormAction::CreateTask { parent } => {
                // task forms always have a `due` date field
                let Some(due) = due else {
                    return;
                };
                let node =
                    Node::task(v.name.clone(), Task::new(dir, due)).with_description(description);
                self.core
                    .create(parent, node)
                    .await
                    .map(|path| (path, format!("added task {}", v.name)))
            }
            FormAction::CreateContainer { parent } => {
                // container forms always have a `kind` field
                let Some(kind) = v.kind else {
                    return;
                };
                // auto without a name: say what's missing
                let Some(dir) = dir else {
                    self.error("name cannot be empty");
                    return;
                };
                let node = Node::container(v.name.clone(), Container::new(dir, kind))
                    .with_description(description);
                self.core
                    .create(parent, node)
                    .await
                    .map(|path| (path, format!("created {kind} {}", v.name)))
            }
            FormAction::EditNode { path } => {
                // always sends every field: an unchanged name passes the
                // checks, an emptied description is removed by the tree
                let body = match (due, v.kind) {
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
                let patch = NodePatch {
                    header: HeaderPatch {
                        name: Some(v.name.clone()),
                        description: Some(description),
                    },
                    body,
                };
                self.core
                    .edit(path, patch)
                    .await
                    .map(|()| (path.clone(), format!("saved {}", v.name)))
            }
            FormAction::EditSettings { path } => {
                let (settings, root) = match form.settings() {
                    Ok(s) => s,
                    Err(e) => {
                        self.error(e);
                        return;
                    }
                };
                // opened from a task: the cursor stays on the task
                let cursor = self.tree_state.cursor.clone();
                self.core
                    .set_settings(path, settings, root)
                    .await
                    .map(|()| (cursor, "saved settings".to_string()))
            }
            FormAction::EditSession { .. } => return, // saved at the top
        };

        match saved {
            Ok((path, msg)) => {
                self.tree_state.reveal(self.core.tree(), path);
                self.mode = Mode::Normal;
                self.info(msg);
            }
            Err(e) => self.error(e.to_string()),
        }
    }
}

/// Where the keys go when `action`'s form closes: the list it came from
/// (sessions), else the tree.
fn mode_after(action: &FormAction) -> Mode {
    match action {
        FormAction::EditSession { .. } => Mode::Sessions,
        _ => Mode::Normal,
    }
}

/// A date from a form (local, as shown) as a `Time`. A local time a DST
/// switch skips: the error for the toast.
pub(super) fn to_time(local: NaiveDateTime) -> Result<Time, &'static str> {
    local_to_fixed(local).ok_or("that time doesn't exist (DST switch)")
}
