//! Forms: `t` / `T` new task, `c` / `C` new container, `e` edit (on the
//! settings tab: the container's settings; in the sessions list: the
//! session, saved in `sessions`). Opening
//! picks the parent (or the node), the form edits itself
//! (`Form::handle_key`), submit picks one `save_*` by the form's action,
//! which calls the tree (it checks names and creates dirs).

use chrono::Local;
use crossterm::event::KeyEvent;

use super::{App, Flow, Mode};
use crate::{
    Res,
    model::{
        NodePath,
        container::ContainerKind,
        settings::{ContainerSettings, view::SettingInfo},
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
            FormOutcome::Submit => {
                let Mode::Form(form) = std::mem::take(&mut self.mode) else {
                    unreachable!("matched Mode::Form above");
                };
                match self.save(&form).await {
                    Ok(saved) => self.after_save(&form.action, saved),
                    Err(e) => {
                        self.mode = Mode::Form(form);
                        self.error(e.to_string());
                    }
                }
            }
        }
        Flow::Continue
    }

    /// Save `form` by its action. Errors: shown by the caller, the form
    /// stays open. Name rules (empty, `/`, `..`, duplicates) are checked by
    /// the tree.
    async fn save(&mut self, form: &Form) -> Res<Saved> {
        match &form.action {
            FormAction::CreateTask { parent } => self.save_new_task(parent, form).await,
            FormAction::CreateContainer { parent } => self.save_new_container(parent, form).await,
            FormAction::EditNode { path } => self.save_node(path, form).await,
            FormAction::EditSettings { path } => self.save_settings(path, form).await,
            FormAction::EditSession { id } => self.save_session(*id, form).await,
        }
    }

    /// New task under `parent` (`Form::new_task_node`).
    async fn save_new_task(&mut self, parent: &NodePath, form: &Form) -> Res<Saved> {
        let node = form.new_task_node()?;
        // before `create` takes the node
        let msg = format!("added task {}", node.name());
        let path = self.core.create(parent, node).await?;
        Ok(Saved {
            reveal: Some(path),
            msg: Some(msg),
        })
    }

    /// New container under `parent` (`Form::new_container_node`).
    async fn save_new_container(&mut self, parent: &NodePath, form: &Form) -> Res<Saved> {
        let node = form.new_container_node()?;
        let kind = node
            .as_container()
            .expect("new_container_node builds a container")
            .kind;
        let msg = format!("created {kind} {}", node.name());
        let path = self.core.create(parent, node).await?;
        Ok(Saved {
            reveal: Some(path),
            msg: Some(msg),
        })
    }

    /// Edit the node at `path` (`Form::node_edit`).
    async fn save_node(&mut self, path: &NodePath, form: &Form) -> Res<Saved> {
        let patch = form.node_edit()?;
        self.core.edit(path, patch).await?;
        Ok(Saved {
            reveal: Some(path.clone()),
            msg: Some(format!("saved {}", form.name())),
        })
    }

    /// Own settings of the container at `path` (plus `[root]` on the root).
    async fn save_settings(&mut self, path: &NodePath, form: &Form) -> Res<Saved> {
        let (settings, root) = form.settings()?;
        // opened from a task: the cursor stays on the task
        let cursor = self.tree_state.cursor.clone();
        self.core.set_settings(path, settings, root).await?;
        Ok(Saved {
            reveal: Some(cursor),
            msg: Some("saved settings".into()),
        })
    }

    /// A form saved: close it (`mode_after`), select what it saved, say so.
    fn after_save(&mut self, action: &FormAction, saved: Saved) {
        if let Some(path) = saved.reveal {
            self.tree_state.reveal(self.core.tree(), path);
        }
        self.mode = mode_after(action);
        if let Some(msg) = saved.msg {
            self.info(msg);
        }
    }
}

/// What saving a form did. Where the keys go next is `mode_after`, like
/// for `esc`.
pub(super) struct Saved {
    /// Select this node in the tree.
    pub(super) reveal: Option<NodePath>,
    /// Info toast; None: nothing to say (unchanged session form).
    pub(super) msg: Option<String>,
}

/// Where the keys go when `action`'s form closes: the list it came from
/// (sessions), else the tree. Also tells `App::in_list` where an open form
/// belongs.
pub(super) fn mode_after(action: &FormAction) -> Mode {
    match action {
        FormAction::EditSession { .. } => Mode::Sessions,
        _ => Mode::Normal,
    }
}
