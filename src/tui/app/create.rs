//! Create forms: `t` / `T` new task, `c` / `C` new container. Opening picks
//! the parent, the form edits itself (`Form::handle_key`), submit calls the
//! tree (which checks names and creates dirs).

use std::path::PathBuf;

use crossterm::event::KeyEvent;

use super::{App, Flow, Mode};
use crate::{
    model::{
        container::ContainerKind,
        normalize_name,
        task::{Task, local_to_utc},
        tree::NodePath,
    },
    tui::form::{FieldId, Form, FormAction, FormOutcome},
};

impl App<'_> {
    /// Where a new node goes: the root if `global`, else the container at
    /// the cursor (or the task's container).
    fn creation_parent(&self, global: bool) -> NodePath {
        if global {
            return vec![];
        }
        self.tree
            .nearest_file_owner(&self.tree.cursor)
            .unwrap_or_default()
    }

    pub(super) fn open_container_form(&mut self, global: bool) {
        let parent = self.creation_parent(global);
        let parent_node = self.tree.get(&parent);
        let parent_name = parent_node.map_or("root", |n| n.name());
        let parent_dir = parent_node.and_then(|n| n.dir().map(|d| d.to_path_buf()));

        let kind = if parent.is_empty() {
            ContainerKind::Workspace
        } else {
            ContainerKind::Project
        };

        self.mode = Mode::Form(Box::new(Form::new_container(
            parent,
            parent_name,
            parent_dir,
            kind,
        )));
    }

    pub(super) fn open_task_form(&mut self, global: bool) {
        let parent = self.creation_parent(global);
        let parent_name = self.tree.get(&parent).map_or("root", |n| n.name());
        self.mode = Mode::Form(Box::new(Form::new_task(parent, parent_name)));
    }

    pub(super) async fn handle_form_key(&mut self, key: KeyEvent) -> Flow {
        let Mode::Form(form) = &mut self.mode else {
            return Flow::Continue;
        };
        match form.handle_key(key) {
            FormOutcome::Continue => {}
            FormOutcome::Cancel => self.mode = Mode::Normal,
            FormOutcome::Submit => self.submit_form().await,
        }
        Flow::Continue
    }

    /// Create the node. Success: close the form, select the new node. Error:
    /// toast, form stays open so the input can be fixed.
    async fn submit_form(&mut self) {
        let Mode::Form(form) = &self.mode.clone() else {
            return;
        };
        // name rules (empty, `/`, `..`, duplicates) are checked by the tree
        let name = normalize_name(form.text_value(FieldId::Name).unwrap_or(""));

        let created = match &form.action {
            FormAction::CreateTask { parent } => {
                // task forms always have a `due` date field
                let Some(local) = form.date_value(FieldId::Due) else {
                    return;
                };
                let Some(due) = local_to_utc(local) else {
                    self.error("that time doesn't exist (DST switch)");
                    return;
                };
                let dir = self.tree.auto_task_dir(parent, &name);
                let task = Task::new(name.clone(), dir, due);
                self.tree
                    .create_task(parent, task)
                    .await
                    .map(|path| (path, format!("added task {name}")))
            }
            FormAction::CreateContainer { parent, kind } => {
                let dir_str = form.text_value(FieldId::Dir).unwrap_or("").trim();
                let dir = if dir_str.is_empty() {
                    self.tree.default_child_dir(parent, &name)
                } else {
                    Some(PathBuf::from(dir_str))
                };
                let Some(dir) = dir else {
                    self.error("directory cannot be empty");
                    return;
                };
                self.tree
                    .create_container(parent, name.clone(), dir, *kind)
                    .await
                    .map(|path| (path, format!("created {kind} {name}")))
            }
            FormAction::EditNode { .. } => {
                // Future edit support; say so instead of swallowing Enter
                self.error("editing is not supported yet");
                return;
            }
        };

        match created {
            Ok((path, msg)) => {
                self.tree.reveal(path);
                self.mode = Mode::Normal;
                self.info(msg);
            }
            Err(e) => self.error(e.to_string()),
        }
    }
}
