use std::path::PathBuf;

use tokio::fs;

use crate::{
    Res, UDO_FILE_NAME,
    model::{
        NodePath,
        container::{Container, ContainerKind},
        data::ContainerData,
        node::{Node, NodePatch},
        task::{Task, TaskPatch, TaskStatus},
        tree::Tree,
    },
    naming::folder_name,
};

impl Tree {
    /// Insert `node` as a child of the container at `parent`, returning the new
    /// node's path. Errors if `parent` is not a container. In-memory only: no
    /// checks, no disk. Prefer `create_container` / `create_task`.
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

    pub fn update(&mut self, path: &[usize], patch: NodePatch) -> Res<()> {
        self.get_mut(path)
            .ok_or("no node at the path")?
            .update(patch)
    }

    /// Create a child container under `parent`:
    /// mkdir `dir`, insert, save the new container's file AND the parent's file.
    /// Errors if `parent` is not a container, a sibling already has `name`, or
    /// `dir` already holds a `.udo.toml` (never overwrite existing data).
    pub async fn create_container(
        &mut self,
        parent: &[usize],
        name: String,
        dir: PathBuf,
        kind: ContainerKind,
    ) -> Res<NodePath> {
        self.check_can_add(parent, &name)?;
        self.check_dir_free(&dir)?;
        if dir.join(UDO_FILE_NAME).exists() {
            return Err(format!("{} already contains a udo container", dir.display()).into());
        }

        fs::create_dir_all(&dir).await?;
        let path = self.insert(parent, Node::Container(Container::new(name, dir, kind)))?;

        self.save(&path).await?; // new container's own .udo.toml
        self.save(parent).await?; // parent lists the new dir in `children`
        Ok(path)
    }

    /// Add `task` under container `parent` and save the parent's file.
    /// If `task.dir` is Some, that dir is created. Errors if `parent` is not a
    /// container or a sibling already has the task's name.
    pub async fn create_task(&mut self, parent: &[usize], task: Task) -> Res<NodePath> {
        self.check_can_add(parent, &task.name)?;

        if let Some(dir) = &task.dir {
            self.check_dir_free(dir)?;
            fs::create_dir_all(dir).await?;
        }

        let path = self.insert(parent, Node::Task(task))?;

        self.save(parent).await?;
        Ok(path)
    }

    /// Re-save the nearest file-owning container for `path`.
    pub async fn save(&self, path: &[usize]) -> Res<()> {
        let owner = self.nearest_file_owner(path).ok_or("no node at the path")?;
        match self.get(&owner) {
            Some(Node::Container(c)) => ContainerData::from(c).save(&c.dir).await,
            _ => Err("file owner is not a container".into()),
        }
    }

    /// Unregister the node at `path` and re-save its parent.
    ///
    /// Files on disk are left untouched (a container's dir and `.udo.toml`
    /// stay). Sibling indices shift after removal, so `self.cursor` is fixed up.
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
        self.fix_cursor_after_remove(parent_path, idx);
        self.save(parent_path).await
    }

    pub async fn set_task_status(&mut self, path: &[usize], status: TaskStatus) -> Res<()> {
        if !matches!(self.get(path), Some(Node::Task(_))) {
            return Err("only tasks have a status".into());
        }
        self.update(
            path,
            NodePatch::Task(TaskPatch {
                status: Some(status),
                ..Default::default()
            }),
        )?;
        self.save(path).await
    }

    /// Checks before adding a child: `parent` must be a container and must not
    /// already have a child called `name`. Run before touching the disk.
    fn check_can_add(&self, parent: &[usize], name: &str) -> Res<()> {
        // the folder name is what becomes a path component (task dir,
        // default container dir), so check that instead of the name
        let Some(folder) = folder_name(name) else {
            return Err("name cannot be empty".into());
        };
        if folder == "." || folder == ".." || folder.contains(['/', '\\']) {
            return Err("name cannot be . or .. or contain / or \\".into());
        }
        if !matches!(self.get(parent), Some(Node::Container(_))) {
            return Err("parent missing or not a container".into());
        }
        if self.find_child(parent, name).is_some() {
            return Err(format!("{name:?} already exists here").into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::model::{
        container::{Container, ContainerKind, ContainerSettings},
        data::ContainerData,
        node::{Node, NodePatch},
        task::{TaskPatch, TaskStatus},
        tree::{
            Tree,
            tests::{container, disk_tree, new_task, task, tree},
        },
    };

    #[test]
    fn update_edits_node_at_path() {
        let mut t = tree();
        t.update(
            &[1, 0],
            NodePatch::Task(TaskPatch {
                name: Some("z".into()),
                ..Default::default()
            }),
        )
        .unwrap();
        assert_eq!(t.get(&[1, 0]).unwrap().name(), "z");
    }

    #[test]
    fn update_errors_on_missing_path() {
        let mut t = tree();
        assert!(
            t.update(&[9], NodePatch::Task(TaskPatch::default()))
                .is_err()
        );
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
        let t = Tree {
            root: Node::Container(Container {
                name: "root".into(),
                dir: dir.path().to_path_buf(),
                kind: ContainerKind::Root,
                settings: ContainerSettings::default(),
                unloaded: vec![],
                collapsed: false,
                children: vec![task("a"), container("inner", vec![task("b")])],
            }),
            cursor: vec![],
        };

        t.save(&[]).await.unwrap();

        // root's file lists its task rows + its child container dirs (not nested content)
        let data = ContainerData::load(dir.path()).await.unwrap();
        assert_eq!(data.name, "root");
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
        let mut t = disk_tree(tmp.path()).await;

        t.delete(&[0]).await.unwrap();

        assert_eq!(t.get(&[0]).unwrap().name(), "ws");
        let data = ContainerData::load(tmp.path()).await.unwrap();
        assert!(data.tasks.is_empty());
        assert_eq!(data.children, vec![tmp.path().join("ws")]);
    }

    #[tokio::test]
    async fn delete_container_unregisters_but_keeps_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let mut t = disk_tree(tmp.path()).await;

        t.delete(&[1]).await.unwrap();

        assert!(t.get(&[1]).is_none());
        let data = ContainerData::load(tmp.path()).await.unwrap();
        assert!(data.children.is_empty());
        // user files are never touched
        assert!(tmp.path().join("ws").join(crate::UDO_FILE_NAME).exists());
    }

    // ---------- create_container ----------

    #[tokio::test]
    async fn create_container_makes_dir_and_saves_both_files() {
        let tmp = tempfile::tempdir().unwrap();
        let mut t = Tree::load_from(tmp.path()).await.unwrap(); // empty root
        let ws_dir = tmp.path().join("ws");

        let p = t
            .create_container(&[], "ws".into(), ws_dir.clone(), ContainerKind::Workspace)
            .await
            .unwrap();

        assert_eq!(p, vec![0]);
        assert_eq!(t.get(&p).unwrap().name(), "ws");
        // new container's own file
        let ws = ContainerData::load(&ws_dir).await.unwrap();
        assert_eq!(ws.name, "ws");
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

        let ws = t
            .create_container(&[], "ws".into(), ws_dir, ContainerKind::Workspace)
            .await
            .unwrap();
        t.create_container(&ws, "proj".into(), proj_dir, ContainerKind::Project)
            .await
            .unwrap();

        let loaded = Tree::load_from(tmp.path()).await.unwrap();
        assert_eq!(loaded.resolve(&["ws", "proj"]), Some(vec![0, 0]));
    }

    #[tokio::test]
    async fn create_container_rejects_duplicate_name() {
        let tmp = tempfile::tempdir().unwrap();
        let mut t = Tree::load_from(tmp.path()).await.unwrap();

        t.create_container(
            &[],
            "ws".into(),
            tmp.path().join("ws"),
            ContainerKind::Workspace,
        )
        .await
        .unwrap();
        let dup = t
            .create_container(
                &[],
                "ws".into(),
                tmp.path().join("ws2"),
                ContainerKind::Workspace,
            )
            .await;

        assert!(dup.is_err());
        assert_eq!(t.get(&[]).unwrap().children().len(), 1);
        assert!(!tmp.path().join("ws2").exists()); // check happens before mkdir
    }

    #[tokio::test]
    async fn create_container_rejects_task_parent() {
        let tmp = tempfile::tempdir().unwrap();
        let mut t = disk_tree(tmp.path()).await; // [0] is task "a"

        let r = t
            .create_container(
                &[0],
                "x".into(),
                tmp.path().join("x"),
                ContainerKind::Project,
            )
            .await;

        assert!(r.is_err());
        assert!(!tmp.path().join("x").exists());
    }

    #[tokio::test]
    async fn create_container_refuses_existing_udo_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let mut t = disk_tree(tmp.path()).await; // tmp/ws already has a .udo.toml

        let r = t
            .create_container(
                &[],
                "other".into(),
                tmp.path().join("ws"),
                ContainerKind::Workspace,
            )
            .await;

        assert!(r.is_err());
        let ws = ContainerData::load(&tmp.path().join("ws")).await.unwrap();
        assert_eq!(ws.name, "ws"); // untouched
    }

    #[tokio::test]
    async fn create_container_refuses_root_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let mut t = Tree::load_from(tmp.path()).await.unwrap();

        let r = t
            .create_container(
                &[],
                "ws".into(),
                tmp.path().to_path_buf(),
                ContainerKind::Workspace,
            )
            .await;

        assert!(r.is_err());
        assert_eq!(ContainerData::load(tmp.path()).await.unwrap().name, "root");
    }

    // ---------- create_task ----------

    #[tokio::test]
    async fn create_task_saves_row_in_parent_file() {
        let tmp = tempfile::tempdir().unwrap();
        let mut t = disk_tree(tmp.path()).await; // root: [a, ws: [b]]

        let p = t.create_task(&[1], new_task("c", None)).await.unwrap();

        assert_eq!(p, vec![1, 1]);
        assert_eq!(t.get(&p).unwrap().name(), "c");
        let ws = ContainerData::load(&tmp.path().join("ws")).await.unwrap();
        let names: Vec<_> = ws.tasks.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, vec!["b", "c"]);
    }

    #[tokio::test]
    async fn create_task_creates_its_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let mut t = disk_tree(tmp.path()).await;
        let task_dir = tmp.path().join("ws").join("c");

        t.create_task(&[1], new_task("c", Some(task_dir.clone())))
            .await
            .unwrap();

        assert!(task_dir.is_dir());
    }

    #[tokio::test]
    async fn create_task_rejects_duplicate_name() {
        let tmp = tempfile::tempdir().unwrap();
        let mut t = disk_tree(tmp.path()).await;

        assert!(t.create_task(&[1], new_task("b", None)).await.is_err()); // "b" exists in ws
        assert!(t.create_task(&[], new_task("ws", None)).await.is_err()); // clashes with container
        assert_eq!(t.get(&[1]).unwrap().children().len(), 1);
    }

    #[tokio::test]
    async fn create_task_rejects_task_parent() {
        let tmp = tempfile::tempdir().unwrap();
        let mut t = disk_tree(tmp.path()).await;

        assert!(t.create_task(&[0], new_task("x", None)).await.is_err()); // [0] is task "a"
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
            name: "root".into(),
            kind: ContainerKind::Root,
            tasks: vec![],
            children: vec![gone.clone()],
            settings: ContainerSettings::default(),
        }
        .save(&root_dir)
        .await
        .unwrap();

        let mut t = Tree::load_from(&root_dir).await.unwrap();
        t.create_task(&[], new_task("x", None)).await.unwrap(); // re-saves root

        let data = ContainerData::load(&root_dir).await.unwrap();
        assert_eq!(data.children, vec![gone]);
    }

    // ---------- set_task_status ----------

    #[tokio::test]
    async fn set_task_status_updates_tree_and_file() {
        let tmp = tempfile::tempdir().unwrap();
        let mut t = disk_tree(tmp.path()).await; // root: [a, ws: [b]]

        t.set_task_status(&[1, 0], TaskStatus::Finished)
            .await
            .unwrap();

        let Some(Node::Task(b)) = t.get(&[1, 0]) else {
            panic!("expected a task at [1, 0]");
        };
        assert_eq!(b.status, TaskStatus::Finished);
        assert_eq!(b.name, "b"); // nothing else changed

        let ws = ContainerData::load(&tmp.path().join("ws")).await.unwrap();
        assert_eq!(ws.tasks[0].status, TaskStatus::Finished);
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

    // ---------- name rules ----------

    #[tokio::test]
    async fn create_rejects_names_that_are_not_one_path_component() {
        let tmp = tempfile::tempdir().unwrap();
        let mut t = disk_tree(tmp.path()).await; // root: [a, ws: [b]]

        for bad in ["", ".", "..", "a/b", "../x", "a\\b"] {
            assert!(
                t.create_task(&[1], new_task(bad, None)).await.is_err(),
                "task {bad:?} accepted"
            );
            let dir = tmp.path().join("ws").join("dir");
            assert!(
                t.create_container(&[1], bad.into(), dir, ContainerKind::Project)
                    .await
                    .is_err(),
                "container {bad:?} accepted"
            );
        }
        assert_eq!(t.get(&[1]).unwrap().children().len(), 1); // still only "b"
        assert!(!tmp.path().join("ws").join("dir").exists()); // check before mkdir
    }
}
