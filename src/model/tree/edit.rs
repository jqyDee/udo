use tokio::fs;

use crate::{
    Res, UDO_FILE_NAME,
    model::{
        NodePath,
        container::ContainerPatch,
        data::ContainerData,
        node::{BodyPatch, Node, NodePatch, clean_description},
        settings::{ContainerSettings, RootSettings},
        task::{TaskPatch, TaskStatus},
        tree::Tree,
    },
    naming::folder_name,
};

impl Tree {
    /// Insert `node` as a child of the container at `parent`, returning the new
    /// node's path. Errors if `parent` is not a container. In-memory only: no
    /// checks, no disk. Prefer `create`.
    pub fn insert(&mut self, parent: &[usize], node: Node) -> Res<NodePath> {
        let children = self
            .get_mut(parent)
            .and_then(|n| n.children_mut())
            .ok_or("parent missing or not a container")?;
        let idx = children.len();
        children.push(node);
        let mut path = parent.to_vec();
        path.push(idx);
        Ok(path)
    }

    /// Apply `patch` to the node at `path`. In-memory only: no checks, no
    /// disk. Prefer `edit`.
    pub fn update(&mut self, path: &[usize], patch: NodePatch) -> Res<()> {
        self.get_mut(path)
            .ok_or("no node at the path")?
            .update(patch)
    }

    /// Add `node` (task or container) under container `parent`: check name
    /// and dir, mkdir its dir (if any), insert, save the new container's own
    /// file (if it is one) AND the parent's file.
    ///
    /// The description is trimmed; empty -> None. Errors if `parent` is not a
    /// container, a sibling already has the name, the dir is used by another
    /// node, a new container's dir already holds a `.udo.toml` (never
    /// overwrite existing data), or a new container has children (only its
    /// own file would be written).
    pub async fn create(&mut self, parent: &[usize], mut node: Node) -> Res<NodePath> {
        self.check_name(parent, &node.header.name, None)?;
        if !node.children().is_empty() {
            return Err("a new container must not have children".into());
        }
        node.header.description = node.header.description.take().and_then(clean_description);

        if let Some(dir) = node.dir() {
            self.check_dir_free(dir)?;
            if node.owns_file() && dir.join(UDO_FILE_NAME).exists() {
                return Err(format!("{} already contains a udo container", dir.display()).into());
            }
            fs::create_dir_all(dir).await?;
        }

        let owns_file = node.owns_file();
        let path = self.insert(parent, node)?;
        if owns_file {
            self.save(&path).await?; // new container's own .udo.toml
        }
        self.save(parent).await?; // parent lists the new task row / container dir
        Ok(path)
    }

    /// Edit the node at `path` and save its file. A new name gets the same
    /// checks as in `create` (against the siblings, not itself); the
    /// description is cleaned (trim, blank -> None). Errors, before anything
    /// changes, for the root, a missing path, a bad name, a patch of the
    /// other kind, or a dir change (moving folders is not supported).
    pub async fn edit(&mut self, path: &[usize], mut patch: NodePatch) -> Res<()> {
        let (&idx, parent) = path.split_last().ok_or("the root cannot be edited")?;
        if self.get(path).is_none() {
            return Err("no node at the path".into());
        }
        let changes_dir = match &patch.body {
            Some(BodyPatch::Container(p)) => p.dir.is_some(),
            Some(BodyPatch::Task(p)) => p.dir.is_some(),
            None => false,
        };
        if changes_dir {
            return Err("changing a dir is not supported yet".into());
        }
        if let Some(name) = &patch.header.name {
            self.check_name(parent, name, Some(idx))?;
        }
        if let Some(desc) = patch.header.description.take() {
            patch.header.description = Some(desc.and_then(clean_description));
        }

        self.update(path, patch)?;
        self.save(path).await
    }

    /// Replace the own settings of the container at `path` (the root too) and
    /// save its file. `root`: the `[root]` settings, only for the root.
    /// Errors, before anything changes, for a task, a missing path, or
    /// `root` given for another container.
    pub async fn set_settings(
        &mut self,
        path: &[usize],
        settings: ContainerSettings,
        root: Option<RootSettings>,
    ) -> Res<()> {
        if self.get(path).and_then(Node::as_container).is_none() {
            return Err("only containers have settings".into());
        }
        if root.is_some() && !path.is_empty() {
            return Err("root settings only exist on the root".into());
        }
        self.update(
            path,
            NodePatch {
                body: Some(BodyPatch::Container(ContainerPatch {
                    settings: Some(settings),
                    root_settings: root,
                    ..Default::default()
                })),
                ..Default::default()
            },
        )?;
        self.save(path).await
    }

    /// Re-save the nearest file-owning container for `path`.
    pub async fn save(&self, path: &[usize]) -> Res<()> {
        let owner = self.nearest_file_owner(path).ok_or("no node at the path")?;
        let node = self.get(&owner).ok_or("no node at the path")?;
        let c = node.as_container().ok_or("file owner is not a container")?;
        ContainerData::try_from(node)?.save(&c.dir).await
    }

    /// Unregister the node at `path` and re-save its parent.
    ///
    /// Files on disk are left untouched (a container's dir and `.udo.toml`
    /// stay). Sibling indices shift after removal: callers holding paths fix
    /// them up (e.g. `TreeState::after_remove`).
    pub async fn delete(&mut self, path: &[usize]) -> Res<()> {
        let (&idx, parent_path) = path.split_last().ok_or("Root is not deletable!")?;
        let children = self
            .get_mut(parent_path)
            .and_then(|n| n.children_mut())
            .ok_or("parent missing or not a container")?;

        if idx >= children.len() {
            return Err("no node at the path".into());
        }

        children.remove(idx);
        self.save(parent_path).await
    }

    pub async fn set_task_status(&mut self, path: &[usize], status: TaskStatus) -> Res<()> {
        if self.get(path).and_then(Node::as_task).is_none() {
            return Err("only tasks have a status".into());
        }
        self.update(
            path,
            NodePatch {
                body: Some(BodyPatch::Task(TaskPatch {
                    status: Some(status),
                    ..Default::default()
                })),
                ..Default::default()
            },
        )?;
        self.save(path).await
    }

    /// Name checks for a child of `parent` (new or renamed): `name` must work as
    /// one path component, `parent` must be a container, and no other child
    /// may be called `name`. `except` is the index of a child to skip (the
    /// node being renamed). Run before touching the disk.
    fn check_name(&self, parent: &[usize], name: &str, except: Option<usize>) -> Res<()> {
        // the folder name is what becomes a path component (task dir,
        // default container dir), so check that instead of the name
        let Some(folder) = folder_name(name) else {
            return Err("name cannot be empty".into());
        };
        if folder == "." || folder == ".." || folder.contains(['/', '\\']) {
            return Err("name cannot be . or .. or contain / or \\".into());
        }
        if self.get(parent).and_then(Node::as_container).is_none() {
            return Err("parent missing or not a container".into());
        }
        if let Some(found) = self.find_child(parent, name)
            && found.last().copied() != except
        {
            return Err(format!("{name:?} already exists here").into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::{
        model::{
            container::ContainerKind,
            data::ContainerData,
            node::{BodyPatch, HeaderPatch, NodeHeader, NodePatch},
            settings::{ContainerSettings, RootSettings},
            task::{TaskPatch, TaskStatus},
            tree::{Tree, tests::tree},
        },
        test_util::{container, container_at, disk_tree_in, new_container, new_task, task},
    };

    // ---------- create ----------

    #[tokio::test]
    async fn create_trims_description_and_saves_it() {
        let tmp = tempfile::tempdir().unwrap();
        let mut t = disk_tree_in(tmp.path()).await; // root: [a, ws: [b]]
        let node = new_task("c", None).with_description(Some("  two\nlines  ".into()));

        let p = t.create(&[1], node).await.unwrap();

        assert_eq!(t.get(&p).unwrap().header.description.as_deref(), Some("two\nlines"));
        let ws = ContainerData::load(&tmp.path().join("ws")).await.unwrap();
        assert_eq!(ws.tasks[1].header.description.as_deref(), Some("two\nlines"));
    }

    #[tokio::test]
    async fn create_turns_blank_description_into_none() {
        let tmp = tempfile::tempdir().unwrap();
        let mut t = disk_tree_in(tmp.path()).await;
        let node = new_task("c", None).with_description(Some(" \n ".into()));

        let p = t.create(&[1], node).await.unwrap();

        assert_eq!(t.get(&p).unwrap().header.description, None);
    }

    #[tokio::test]
    async fn create_container_with_description() {
        let tmp = tempfile::tempdir().unwrap();
        let mut t = Tree::load_from(tmp.path()).await.unwrap();
        let ws_dir = tmp.path().join("ws");
        let node = container_at("ws", &ws_dir, ContainerKind::Workspace, vec![])
            .with_description(Some("uni stuff".into()));

        t.create(&[], node).await.unwrap();

        let ws = ContainerData::load(&ws_dir).await.unwrap();
        assert_eq!(ws.header.description.as_deref(), Some("uni stuff"));
    }

    #[tokio::test]
    async fn create_rejects_container_with_children_before_mkdir() {
        let tmp = tempfile::tempdir().unwrap();
        let mut t = Tree::load_from(tmp.path()).await.unwrap();
        let ws_dir = tmp.path().join("ws");
        let node = container_at("ws", &ws_dir, ContainerKind::Workspace, vec![task("x")]);

        assert!(t.create(&[], node).await.is_err());
        assert!(!ws_dir.exists());
        assert!(t.get(&[]).unwrap().children().is_empty());
    }

    // ---------- edit (disk_tree: root: [a, ws: [b]]) ----------

    fn rename(name: &str) -> HeaderPatch {
        HeaderPatch {
            name: Some(name.into()),
            ..Default::default()
        }
    }

    fn header_patch(header: HeaderPatch) -> NodePatch {
        NodePatch {
            header,
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn edit_renames_task_in_parent_file() {
        let tmp = tempfile::tempdir().unwrap();
        let mut t = disk_tree_in(tmp.path()).await;

        t.edit(&[1, 0], header_patch(rename("b2"))).await.unwrap();

        assert_eq!(t.get(&[1, 0]).unwrap().name(), "b2");
        let ws = ContainerData::load(&tmp.path().join("ws")).await.unwrap();
        assert_eq!(ws.tasks[0].header.name, "b2");
    }

    #[tokio::test]
    async fn edit_renames_container_in_own_file_and_keeps_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let mut t = disk_tree_in(tmp.path()).await;

        t.edit(&[1], header_patch(rename("Uni WS26")))
            .await
            .unwrap();

        let ws_dir = tmp.path().join("ws"); // folder not renamed
        assert_eq!(t.get(&[1]).unwrap().dir(), Some(ws_dir.as_path()));
        let ws = ContainerData::load(&ws_dir).await.unwrap();
        assert_eq!(ws.header.name, "Uni WS26");
    }

    #[tokio::test]
    async fn edit_keeps_own_name_but_rejects_a_siblings() {
        let tmp = tempfile::tempdir().unwrap();
        let mut t = disk_tree_in(tmp.path()).await;

        t.edit(&[0], header_patch(rename("a"))).await.unwrap(); // unchanged: fine
        assert!(t.edit(&[0], header_patch(rename("ws"))).await.is_err()); // sibling
        assert_eq!(t.get(&[0]).unwrap().name(), "a");
    }

    #[tokio::test]
    async fn edit_rejects_bad_names_root_and_missing_paths() {
        let tmp = tempfile::tempdir().unwrap();
        let mut t = disk_tree_in(tmp.path()).await;

        for bad in ["", "..", "a/b"] {
            assert!(t.edit(&[0], header_patch(rename(bad))).await.is_err(), "{bad:?}");
        }
        assert!(t.edit(&[], header_patch(rename("x"))).await.is_err()); // root
        assert!(t.edit(&[9], NodePatch::default()).await.is_err());
        assert_eq!(t.get(&[0]).unwrap().name(), "a");
    }

    #[tokio::test]
    async fn edit_cleans_sets_and_removes_description() {
        let tmp = tempfile::tempdir().unwrap();
        let mut t = disk_tree_in(tmp.path()).await;
        let desc = |d: Option<&str>| {
            header_patch(HeaderPatch {
                description: Some(d.map(String::from)),
                ..Default::default()
            })
        };
        let saved = async || {
            let ws = ContainerData::load(&tmp.path().join("ws")).await.unwrap();
            ws.tasks[0].header.description.clone()
        };

        t.edit(&[1, 0], desc(Some("  notes "))).await.unwrap();
        assert_eq!(saved().await.as_deref(), Some("notes"));
        t.edit(&[1, 0], desc(Some("   "))).await.unwrap(); // blank = remove
        assert_eq!(saved().await, None);
        t.edit(&[1, 0], desc(Some("x"))).await.unwrap();
        t.edit(&[1, 0], desc(None)).await.unwrap(); // explicit remove
        assert_eq!(saved().await, None);
    }

    #[tokio::test]
    async fn edit_rejects_dir_changes() {
        let tmp = tempfile::tempdir().unwrap();
        let mut t = disk_tree_in(tmp.path()).await;
        let patch = NodePatch {
            body: Some(BodyPatch::Task(TaskPatch {
                dir: Some(tmp.path().join("elsewhere")),
                ..Default::default()
            })),
            ..Default::default()
        };

        assert!(t.edit(&[1, 0], patch).await.is_err());
        assert_eq!(t.get(&[1, 0]).unwrap().dir(), None);
    }

    #[test]
    fn update_edits_node_at_path() {
        let mut t = tree();
        t.update(
            &[1, 0],
            NodePatch {
                header: rename("z"),
                body: Some(BodyPatch::Task(TaskPatch {
                    status: Some(TaskStatus::Finished),
                    ..Default::default()
                })),
            },
        )
        .unwrap();
        let b = t.get(&[1, 0]).unwrap();
        assert_eq!(b.name(), "z");
        assert_eq!(b.as_task().unwrap().status, TaskStatus::Finished);
    }

    #[test]
    fn update_errors_on_missing_path() {
        let mut t = tree();
        assert!(t.update(&[9], NodePatch::default()).is_err());
    }

    #[test]
    fn insert_adds_child_and_returns_its_path() {
        let mut t = tree();
        let p = t.insert(&[1], task("c")).unwrap(); // into "inner" (has 1 child already)
        assert_eq!(p, vec![1, 1]); // lands after "b"
        assert_eq!(t.get(&[1, 1]).unwrap().name(), "c");
    }

    #[test]
    fn insert_errors_when_parent_is_a_task() {
        let mut t = tree();
        assert!(t.insert(&[0], task("c")).is_err()); // [0] is a task — no children
    }

    #[tokio::test]
    async fn save_writes_owner_container_file() {
        let dir = tempfile::tempdir().unwrap();
        let t = Tree::new(container_at(
            "root",
            dir.path(),
            ContainerKind::Root,
            vec![task("a"), container("inner", vec![task("b")])],
        ));

        t.save(&[]).await.unwrap();

        // root's file lists its task rows + its child container dirs (not nested content)
        let data = ContainerData::load(dir.path()).await.unwrap();
        assert_eq!(data.header.name, "root");
        assert_eq!(data.tasks.len(), 1); // task "a"
        assert_eq!(data.children, vec![PathBuf::from("/tmp/inner")]); // subcontainer dir only
    }

    #[tokio::test]
    async fn save_errors_on_missing_path() {
        let t = tree(); // errors before writing, so /tmp is never touched
        assert!(t.save(&[9]).await.is_err());
        assert!(t.save(&[1, 5]).await.is_err());
    }

    // ---------- delete ----------

    #[tokio::test]
    async fn delete_rejects_root_and_bad_paths() {
        // all fail before any save, so the /tmp dirs of tree() are never written
        let mut t = tree();
        assert!(t.delete(&[]).await.is_err()); // root
        assert!(t.delete(&[9]).await.is_err()); // out of range
        assert!(t.delete(&[0, 0]).await.is_err()); // parent [0] is a task
    }

    #[tokio::test]
    async fn delete_task_drops_row_from_parent_file() {
        let tmp = tempfile::tempdir().unwrap();
        let mut t = disk_tree_in(tmp.path()).await;

        t.delete(&[0]).await.unwrap();

        assert_eq!(t.get(&[0]).unwrap().name(), "ws");
        let data = ContainerData::load(tmp.path()).await.unwrap();
        assert!(data.tasks.is_empty());
        assert_eq!(data.children, vec![tmp.path().join("ws")]);
    }

    #[tokio::test]
    async fn delete_container_unregisters_but_keeps_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let mut t = disk_tree_in(tmp.path()).await;

        t.delete(&[1]).await.unwrap();

        assert!(t.get(&[1]).is_none());
        let data = ContainerData::load(tmp.path()).await.unwrap();
        assert!(data.children.is_empty());
        // user files are never touched
        assert!(tmp.path().join("ws").join(crate::UDO_FILE_NAME).exists());
    }

    // ---------- create: containers ----------

    #[tokio::test]
    async fn create_container_makes_dir_and_saves_both_files() {
        let tmp = tempfile::tempdir().unwrap();
        let mut t = Tree::load_from(tmp.path()).await.unwrap(); // empty root
        let ws_dir = tmp.path().join("ws");

        let ws = new_container("ws", &ws_dir, ContainerKind::Workspace);
        let p = t.create(&[], ws).await.unwrap();

        assert_eq!(p, vec![0]);
        assert_eq!(t.get(&p).unwrap().name(), "ws");
        // new container's own file
        let ws = ContainerData::load(&ws_dir).await.unwrap();
        assert_eq!(ws.header.name, "ws");
        assert_eq!(ws.kind, ContainerKind::Workspace);
        // parent lists it
        let root = ContainerData::load(tmp.path()).await.unwrap();
        assert_eq!(root.children, vec![ws_dir]);
    }

    #[tokio::test]
    async fn create_container_nested_survives_reload() {
        let tmp = tempfile::tempdir().unwrap();
        let mut t = Tree::load_from(tmp.path()).await.unwrap();
        let ws_dir = tmp.path().join("ws");
        let proj_dir = ws_dir.join("proj");

        let ws = new_container("ws", &ws_dir, ContainerKind::Workspace);
        let ws = t.create(&[], ws).await.unwrap();
        let proj = new_container("proj", &proj_dir, ContainerKind::Project);
        t.create(&ws, proj).await.unwrap();

        let loaded = Tree::load_from(tmp.path()).await.unwrap();
        assert_eq!(loaded.resolve(&["ws", "proj"]), Some(vec![0, 0]));
    }

    #[tokio::test]
    async fn create_container_rejects_duplicate_name() {
        let tmp = tempfile::tempdir().unwrap();
        let mut t = Tree::load_from(tmp.path()).await.unwrap();

        let ws = new_container("ws", &tmp.path().join("ws"), ContainerKind::Workspace);
        t.create(&[], ws).await.unwrap();
        let dup = new_container("ws", &tmp.path().join("ws2"), ContainerKind::Workspace);
        let dup = t.create(&[], dup).await;

        assert!(dup.is_err());
        assert_eq!(t.get(&[]).unwrap().children().len(), 1);
        assert!(!tmp.path().join("ws2").exists()); // check happens before mkdir
    }

    #[tokio::test]
    async fn create_container_rejects_task_parent() {
        let tmp = tempfile::tempdir().unwrap();
        let mut t = disk_tree_in(tmp.path()).await; // [0] is task "a"

        let x = new_container("x", &tmp.path().join("x"), ContainerKind::Project);
        let r = t.create(&[0], x).await;

        assert!(r.is_err());
        assert!(!tmp.path().join("x").exists());
    }

    #[tokio::test]
    async fn create_container_refuses_existing_udo_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let mut t = disk_tree_in(tmp.path()).await; // tmp/ws already has a .udo.toml

        let other = new_container("other", &tmp.path().join("ws"), ContainerKind::Workspace);
        let r = t.create(&[], other).await;

        assert!(r.is_err());
        let ws = ContainerData::load(&tmp.path().join("ws")).await.unwrap();
        assert_eq!(ws.header.name, "ws"); // untouched
    }

    #[tokio::test]
    async fn create_container_refuses_root_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let mut t = Tree::load_from(tmp.path()).await.unwrap();

        let ws = new_container("ws", tmp.path(), ContainerKind::Workspace);
        let r = t.create(&[], ws).await;

        assert!(r.is_err());
        let root = ContainerData::load(tmp.path()).await.unwrap();
        assert_eq!(root.header.name, "root");
    }

    // ---------- create: tasks ----------

    #[tokio::test]
    async fn create_task_saves_row_in_parent_file() {
        let tmp = tempfile::tempdir().unwrap();
        let mut t = disk_tree_in(tmp.path()).await; // root: [a, ws: [b]]

        let p = t.create(&[1], new_task("c", None)).await.unwrap();

        assert_eq!(p, vec![1, 1]);
        assert_eq!(t.get(&p).unwrap().name(), "c");
        let ws = ContainerData::load(&tmp.path().join("ws")).await.unwrap();
        let names: Vec<_> = ws.tasks.iter().map(|t| t.header.name.as_str()).collect();
        assert_eq!(names, vec!["b", "c"]);
    }

    #[tokio::test]
    async fn create_task_creates_its_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let mut t = disk_tree_in(tmp.path()).await;
        let task_dir = tmp.path().join("ws").join("c");

        t.create(&[1], new_task("c", Some(task_dir.clone())))
            .await
            .unwrap();

        assert!(task_dir.is_dir());
    }

    #[tokio::test]
    async fn create_task_rejects_duplicate_name() {
        let tmp = tempfile::tempdir().unwrap();
        let mut t = disk_tree_in(tmp.path()).await;

        assert!(t.create(&[1], new_task("b", None)).await.is_err()); // "b" exists in ws
        assert!(t.create(&[], new_task("ws", None)).await.is_err()); // clashes with container
        assert_eq!(t.get(&[1]).unwrap().children().len(), 1);
    }

    #[tokio::test]
    async fn create_task_rejects_task_parent() {
        let tmp = tempfile::tempdir().unwrap();
        let mut t = disk_tree_in(tmp.path()).await;

        assert!(t.create(&[0], new_task("x", None)).await.is_err()); // [0] is task "a"
    }

    // ---------- regressions ----------

    /// A child that can't be loaded (e.g. drive unmounted) must stay
    /// registered when its parent is saved again.
    #[tokio::test]
    async fn save_keeps_unloaded_child_registered() {
        let tmp = tempfile::tempdir().unwrap();
        let root_dir = tmp.path().to_path_buf();
        let gone = root_dir.join("gone");
        ContainerData {
            header: NodeHeader::new("root".into()),
            kind: ContainerKind::Root,
            tasks: vec![],
            children: vec![gone.clone()],
            settings: ContainerSettings::default(),
            root: RootSettings::default(),
        }
        .save(&root_dir)
        .await
        .unwrap();

        let mut t = Tree::load_from(&root_dir).await.unwrap();
        t.create(&[], new_task("x", None)).await.unwrap(); // re-saves root

        let data = ContainerData::load(&root_dir).await.unwrap();
        assert_eq!(data.children, vec![gone]);
    }

    // ---------- set_task_status ----------

    #[tokio::test]
    async fn set_task_status_updates_tree_and_file() {
        let tmp = tempfile::tempdir().unwrap();
        let mut t = disk_tree_in(tmp.path()).await; // root: [a, ws: [b]]

        t.set_task_status(&[1, 0], TaskStatus::Finished)
            .await
            .unwrap();

        let b = t.get(&[1, 0]).unwrap();
        assert_eq!(b.as_task().unwrap().status, TaskStatus::Finished);
        assert_eq!(b.name(), "b"); // nothing else changed

        let ws = ContainerData::load(&tmp.path().join("ws")).await.unwrap();
        assert_eq!(ws.tasks[0].task.status, TaskStatus::Finished);
    }

    #[tokio::test]
    async fn set_task_status_rejects_container() {
        let mut t = tree(); // in memory: root: [a, inner: [b]]
        let err = t
            .set_task_status(&[1], TaskStatus::Finished)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("only tasks"));
    }

    #[tokio::test]
    async fn set_task_status_rejects_missing_path() {
        let mut t = tree();
        assert!(t.set_task_status(&[9], TaskStatus::Finished).await.is_err());
    }

    // ---------- set_settings (disk_tree: root: [a, ws: [b]]) ----------

    fn deadline(rule: &str) -> ContainerSettings {
        ContainerSettings {
            default_deadline: Some(rule.parse().unwrap()),
            ..Default::default()
        }
    }

    fn theme(name: &str) -> RootSettings {
        RootSettings {
            theme: Some(name.into()),
        }
    }

    #[tokio::test]
    async fn set_settings_saves_the_containers_own_file() {
        let tmp = tempfile::tempdir().unwrap();
        let mut t = disk_tree_in(tmp.path()).await;

        t.set_settings(&[1], deadline("fri 22:00"), None)
            .await
            .unwrap();

        let ws = ContainerData::load(&tmp.path().join("ws")).await.unwrap();
        assert_eq!(ws.settings.default_deadline, Some("fri 22:00".parse().unwrap()));
        let root = ContainerData::load(tmp.path()).await.unwrap();
        assert_eq!(root.settings.default_deadline, None); // parent untouched
    }

    #[tokio::test]
    async fn set_settings_replaces_all_so_unset_fields_are_removed() {
        let tmp = tempfile::tempdir().unwrap();
        let mut t = disk_tree_in(tmp.path()).await;
        t.set_settings(&[1], deadline("fri 22:00"), None)
            .await
            .unwrap();

        t.set_settings(&[1], ContainerSettings::default(), None)
            .await
            .unwrap();

        let ws = ContainerData::load(&tmp.path().join("ws")).await.unwrap();
        assert_eq!(ws.settings.default_deadline, None);
    }

    #[tokio::test]
    async fn set_settings_on_the_root_with_root_settings_survives_reload() {
        let tmp = tempfile::tempdir().unwrap();
        let mut t = disk_tree_in(tmp.path()).await;

        t.set_settings(&[], deadline("+7d 23:59"), Some(theme("dark")))
            .await
            .unwrap();

        let loaded = Tree::load_from(tmp.path()).await.unwrap();
        assert_eq!(loaded.root_settings(), &theme("dark"));
        let root = loaded.get(&[]).unwrap().as_container().unwrap();
        assert_eq!(root.settings.default_deadline, Some("+7d 23:59".parse().unwrap()));
    }

    #[tokio::test]
    async fn set_settings_without_root_settings_keeps_them() {
        let tmp = tempfile::tempdir().unwrap();
        let mut t = disk_tree_in(tmp.path()).await;
        t.set_settings(&[], ContainerSettings::default(), Some(theme("dark")))
            .await
            .unwrap();

        t.set_settings(&[], deadline("fri 22:00"), None)
            .await
            .unwrap();

        assert_eq!(t.root_settings(), &theme("dark"));
    }

    #[tokio::test]
    async fn set_settings_rejects_tasks_missing_paths_and_root_settings_elsewhere() {
        // all fail before any save, so the /tmp dirs of tree() are never written
        let mut t = tree(); // root: [a, inner: [b]]

        assert!(
            t.set_settings(&[0], deadline("fri 22:00"), None)
                .await
                .is_err()
        ); // task
        assert!(
            t.set_settings(&[9], deadline("fri 22:00"), None)
                .await
                .is_err()
        );
        let r = t.set_settings(&[1], deadline("fri 22:00"), Some(theme("x")));
        assert!(r.await.is_err());
        let inner = t.get(&[1]).unwrap().as_container().unwrap();
        assert_eq!(inner.settings.default_deadline, None); // unchanged
        assert_eq!(inner.root_settings, RootSettings::default());
    }

    // ---------- name rules ----------

    #[tokio::test]
    async fn create_rejects_names_that_are_not_one_path_component() {
        let tmp = tempfile::tempdir().unwrap();
        let mut t = disk_tree_in(tmp.path()).await; // root: [a, ws: [b]]

        for bad in ["", ".", "..", "a/b", "../x", "a\\b"] {
            let task = new_task(bad, None);
            assert!(t.create(&[1], task).await.is_err(), "task {bad:?} accepted");
            let dir = tmp.path().join("ws").join("dir");
            let proj = new_container(bad, &dir, ContainerKind::Project);
            assert!(t.create(&[1], proj).await.is_err(), "container {bad:?} accepted");
        }
        assert_eq!(t.get(&[1]).unwrap().children().len(), 1); // still only "b"
        assert!(!tmp.path().join("ws").join("dir").exists()); // check before mkdir
    }
}
