//! Create forms: `t` / `T` new task, `c` / `C` new container. Opening picks
//! the parent, the form edits itself (`Form::handle_key`), submit calls the
//! tree (which checks names and creates dirs).

use chrono::{Local, NaiveTime, TimeDelta, TimeZone, Utc};
use crossterm::event::KeyEvent;

use super::{App, Flow, Mode};
use crate::{
    model::{
        NodePath,
        container::{Container, ContainerKind},
        node::Node,
        task::Task,
    },
    naming::normalize_name,
    tui::form::{FieldId, FolderMode, Form, FormAction, FormOutcome, TaskDefaults},
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
        let parent_node = self.tree.get(&parent);
        let parent_name = parent_node.map_or("root", |n| n.name());
        let parent_dir = parent_node.and_then(|n| n.dir().map(|d| d.to_path_buf()));

        // folders only in projects, until the `task_folders` setting
        let in_project = parent_node
            .and_then(Node::as_container)
            .is_some_and(|c| c.kind == ContainerKind::Project);
        let folder = if in_project {
            FolderMode::Auto
        } else {
            FolderMode::None
        };

        // this has to move into the tree at some point I believe
        let tomorrow_noon = (Local::now().date_naive() + TimeDelta::days(1))
            .and_time(NaiveTime::from_hms_opt(12, 0, 0).unwrap());
        let defaults = TaskDefaults {
            due: tomorrow_noon,
            folder,
        };

        self.mode = Mode::Form(Box::new(Form::new_task(
            parent,
            parent_name,
            parent_dir,
            defaults,
        )));
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
        let description = form.text_value(FieldId::Description).map(str::to_string);

        let created = match &form.action {
            FormAction::CreateTask { parent } => {
                // task forms always have a `due` date field
                let Some(local) = form.date_value(FieldId::Due) else {
                    return;
                };
                let Some(due) = Local
                    .from_local_datetime(&local)
                    .earliest()
                    .map(|t| t.with_timezone(&Utc))
                else {
                    self.error("that time doesn't exist (DST switch)");
                    return;
                };
                // by folder mode; clashes with other nodes are checked by the tree
                let dir = match form.chosen_dir() {
                    Ok(dir) => dir,
                    Err(e) => {
                        self.error(e);
                        return;
                    }
                };

                let node =
                    Node::task(name.clone(), Task::new(dir, due)).with_description(description);

                self.tree
                    .create(parent, node)
                    .await
                    .map(|path| (path, format!("added task {name}")))
            }
            FormAction::CreateContainer { parent } => {
                // container forms always have a `kind` field
                let Some(kind) = form.container_kind() else {
                    return;
                };
                let dir = match form.chosen_dir() {
                    Ok(Some(dir)) => dir,
                    // auto without a name: say what's missing
                    Ok(None) => {
                        self.error("name cannot be empty");
                        return;
                    }
                    Err(e) => {
                        self.error(e);
                        return;
                    }
                };

                let node = Node::container(name.clone(), Container::new(dir, kind))
                    .with_description(description);

                self.tree
                    .create(parent, node)
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
