//! Forms: `t` / `T` new task, `c` / `C` new container, `e` edit. Opening
//! picks the parent (or the node), the form edits itself
//! (`Form::handle_key`), submit calls the tree (which checks names and
//! creates dirs).

use chrono::{Local, NaiveTime, TimeDelta};
use crossterm::event::KeyEvent;

use super::{App, Flow, Mode};
use crate::{
    model::{
        NodePath,
        container::{Container, ContainerKind, ContainerPatch},
        node::{BodyPatch, HeaderPatch, Node, NodePatch},
        task::{Task, TaskPatch},
    },
    tui::form::{Form, FormAction, FormOutcome, TaskDefaults, FolderMode, local_to_utc},
};

impl App<'_> {
    /// Where a new node goes: the root if `global`, else the container at
    /// the cursor (or the task's container).
    fn creation_parent(&self, global: bool) -> NodePath {
        if global {
            return vec![];
        }
        self.tree
            .nearest_file_owner(&self.tree_state.cursor)
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

    /// Edit form for the node at the cursor, prefilled with its values.
    pub(super) fn open_edit_form(&mut self) {
        match self.tree_state.selected(self.tree) {
            Some(node) => {
                let path = self.tree_state.cursor.clone();
                self.mode = Mode::Form(Box::new(Form::edit_node(path, node)));
            }
            None => self.error("nothing selected"),
        }
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

    /// Create or edit the node. The input is read and checked once, then each
    /// action only builds a node (create) or a patch (edit) for the tree.
    /// Success: close the form, select the node. Error: toast, form stays
    /// open so the input can be fixed.
    async fn submit_form(&mut self) {
        let Mode::Form(form) = &self.mode.clone() else {
            return;
        };
        // name rules (empty, `/`, `..`, duplicates) are checked by the tree
        let v = form.values();
        let description = Some(v.description);
        // only task forms have a due date
        let due = match v.due.map(local_to_utc) {
            Some(None) => {
                self.error("that time doesn't exist (DST switch)");
                return;
            }
            due => due.flatten(),
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
                let node = Node::task(v.name.clone(), Task::new(dir, due))
                    .with_description(description);
                self.tree
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
                self.tree
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
                self.tree
                    .edit(path, patch)
                    .await
                    .map(|()| (path.clone(), format!("saved {}", v.name)))
            }
        };

        match saved {
            Ok((path, msg)) => {
                self.tree_state.reveal(self.tree, path);
                self.mode = Mode::Normal;
                self.info(msg);
            }
            Err(e) => self.error(e.to_string()),
        }
    }
}
