use std::path::{Path, PathBuf};

use async_recursion::async_recursion;
use tokio::fs;

use crate::{
    Res, UDO_FILE_NAME, dir::root_dir, model::{
        NodePath,
        container::{Container, ContainerKind},
        data::ContainerData,
        node::{Node, NodePatch},
        task::{Task, TaskPatch, TaskStatus},
        view::ViewState,
    }, naming::folder_name,
};

/// In-memory tree node. Not serialized directly - Persistence goes through DTOs.
pub struct Tree {
    pub root: Node,
    pub cursor: NodePath,
}

impl Tree {
    /// Load the tree from the on-disk file structure.
    pub async fn load() -> Res<Self> {
        Self::load_from(&root_dir()?).await
    }

    pub async fn load_from(root_dir: &Path) -> Res<Self> {
        if !root_dir.join(UDO_FILE_NAME).exists() {
            println!("First run: creating root in {root_dir:?}!");
            fs::create_dir_all(root_dir).await?;

            let root = Container::new("root".into(), root_dir.to_path_buf(), ContainerKind::Root);
            let tree = Self {
                root: Node::Container(root),
                cursor: vec![],
            };
            tree.save(&[]).await?;
            return Ok(tree);
        }
        let mut tree = Self {
            root: Self::build_container(root_dir.to_path_buf()).await?,
            cursor: vec![],
        };
        tree.apply_view(&ViewState::load(root_dir).await);
        Ok(tree)
    }

    /// Recursively build one container from its dir: load DTO, map tasks ->
    /// Node::Task, recurse each child dir -> Node::Container.
    ///
    /// `#[async_recursion]` boxes the future (async fn can't recurse, E0733).
    #[async_recursion]
    async fn build_container(dir: PathBuf) -> Res<Node> {
        let data = ContainerData::load(&dir).await?;

        let mut children: Vec<Node> = data.tasks.into_iter().map(Node::Task).collect();
        let mut unloaded = vec![];

        for child_path in data.children {
            if !child_path.join(UDO_FILE_NAME).exists() {
                eprintln!("warning: could not find a container in location {child_path:?}");
                unloaded.push(child_path); // keep it registered, see Container::unloaded
                continue;
            }

            children.push(Self::build_container(child_path).await?);
        }

        Ok(Node::Container(Container {
            name: data.name,
            dir,
            kind: data.kind,
            settings: data.settings,
            children,
            unloaded,
            collapsed: false,
        }))
    }
}

impl Tree {
    /// Get the node at a given path:
    ///
    /// Path is structured as the children ids from the root node.
    pub fn get(&self, path: &[usize]) -> Option<&Node> {
        let mut cur = &self.root;
        for &i in path {
            cur = cur.children().get(i)?;
        }
        Some(cur)
    }

    /// Get the node at a given path (mut).
    ///
    /// Path is structured as the children ids from the root node.
    pub fn get_mut(&mut self, path: &[usize]) -> Option<&mut Node> {
        let mut cur = &mut self.root;
        for &i in path {
            cur = cur.children_mut()?.get_mut(i)?;
        }
        Some(cur)
    }

    /// Path of the nearest ancestor (or self) that owns a file.
    /// Task -> its parent container; container -> itself; missing -> None.
    pub fn nearest_file_owner(&self, path: &[usize]) -> Option<NodePath> {
        match self.get(path)? {
            Node::Container(_) => Some(path.to_vec()),
            Node::Task(_) => Some(path[..path.len() - 1].to_vec()), // go one up
        }
    }

    /// Path of the direct child of `parent` called `name` (task or container).
    /// None if `parent` is missing, is a task, or has no such child.
    pub fn find_child(&self, parent: &[usize], name: &str) -> Option<NodePath> {
        let parent_node = self.get(parent)?;
        let found_idx = parent_node
            .children()
            .iter()
            .position(|n| n.name() == name)?;
        let mut path = parent.to_vec();
        path.push(found_idx);
        Some(path)
    }

    /// Follow a chain of names from the root, e.g. `["work", "proj-a"]`.
    /// Empty `names` is the root (`Some(vec![])`).
    pub fn resolve(&self, names: &[&str]) -> Option<NodePath> {
        let mut path = vec![];
        for name in names {
            path = self.find_child(&path, name)?;
        }
        Some(path)
    }

    /// Default dir for a new child container: `<parent dir>/<name>`.
    /// None if `parent` is missing or a task.
    pub fn default_child_dir(&self, parent: &[usize], name: &str) -> Option<PathBuf> {
        match self.get(parent)? {
            Node::Container(c) => Some(c.dir.join(folder_name(name)?)),
            Node::Task(_) => None,
        }
    }

    /// Automatic dir for a new task: `<project dir>/<name>` if `parent` is a
    /// Project, None elsewhere (tasks in workspaces / root get no folder).
    pub fn auto_task_dir(&self, parent: &[usize], name: &str) -> Option<PathBuf> {
        match self.get(parent)? {
            Node::Container(c) if c.kind == ContainerKind::Project => {
                Some(c.dir.join(folder_name(name)?))
            }
            _ => None,
        }
    }

    /// Who already uses `dir`, anywhere in the tree. Containers, tasks with a
    /// folder, and `unloaded` child dirs (still registered) count.
    pub fn dir_owner(&self, dir: &Path) -> Option<DirOwner> {
        fn walk(node: &Node, path: &mut NodePath, dir: &Path) -> Option<DirOwner> {
            if node.dir() == Some(dir) {
                return Some(DirOwner::Node(path.clone()));
            }
            let Node::Container(c) = node else {
                return None;
            };
            if c.unloaded.iter().any(|u| u == dir) {
                return Some(DirOwner::Unloaded(path.clone()));
            }
            for (i, child) in c.children.iter().enumerate() {
                path.push(i);
                let found = walk(child, path, dir);
                path.pop();
                if found.is_some() {
                    return found;
                }
            }
            None
        }
        walk(&self.root, &mut vec![], dir)
    }

    /// Error if `dir` is already used by another node (see `dir_owner`).
    fn check_dir_free(&self, dir: &Path) -> Res<()> {
        let owner = match self.dir_owner(dir) {
            None => return Ok(()),
            Some(DirOwner::Node(path)) => {
                let name = self.get(&path).map_or("?", |n| n.name());
                format!("{name:?}")
            }
            Some(DirOwner::Unloaded(parent)) => {
                let name = self.get(&parent).map_or("?", |n| n.name());
                format!("an unloaded container in {name:?}")
            }
        };
        Err(format!("{} is already used by {owner}", dir.display()).into())
    }
}

/// Result of `Tree::dir_owner`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DirOwner {
    /// A loaded node (container or task) at this path.
    Node(NodePath),
    /// A child dir of the container at this path that could not be loaded.
    Unloaded(NodePath),
}

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
}

impl Tree {
    /// Keep `self.cursor` valid after the child at `parent_path + [idx]` was removed.
    fn fix_cursor_after_remove(&mut self, parent_path: &[usize], idx: usize) {
        let depth = parent_path.len();
        // Only cursors below the parent are affected.
        if self.cursor.len() <= depth || !self.cursor.starts_with(parent_path) {
            return;
        }

        let c = self.cursor[depth];
        if c > idx {
            // later sibling (or inside it): shifted left by one
            self.cursor[depth] -= 1;
        } else if c == idx {
            // on the removed node or inside it: take the sibling that moved
            // into the gap, else the previous one, else the parent
            let remaining = self.get(parent_path).map_or(0, |p| p.children().len());
            self.cursor.truncate(depth);
            if remaining > 0 {
                self.cursor.push(idx.min(remaining - 1));
            }
        }
        // c < idx: earlier sibling, unaffected
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
    use std::ffi::OsString;

use chrono::Utc;

    use crate::{
        dir::root_dir_from, model::{container::{ContainerKind, ContainerSettings}, task::{Task, TaskPatch, TaskStatus}},
    };

    use super::*;

    fn task(name: &str) -> Node {
        Node::Task(Task {
            name: name.into(),
            dir: None,
            status: TaskStatus::Pending,
            due_date: Utc::now(),
        })
    }
    fn container(name: &str, children: Vec<Node>) -> Node {
        Node::Container(Container {
            name: name.into(),
            dir: PathBuf::from("/tmp").join(name), // distinct dirs — useful later
            kind: ContainerKind::Workspace,
            settings: ContainerSettings::default(),
            unloaded: vec![],
            collapsed: false,
            children,
        })
    }
    fn tree() -> Tree {
        Tree {
            root: container("root", vec![task("a"), container("inner", vec![task("b")])]),
            cursor: vec![],
        }
    }

    #[test]
    fn tree_get() {
        let tree = tree();
        assert!(tree.get(&[]).is_some());
        assert_eq!(tree.get(&[0]).unwrap().name(), "a");
        assert_eq!(tree.get(&[1]).unwrap().name(), "inner");
        assert_eq!(tree.get(&[1, 0]).unwrap().name(), "b");
        assert!(tree.get(&[9]).is_none());
    }

    #[test]
    fn get_mut() {
        let mut tree = tree();
        tree.get_mut(&[0])
            .unwrap()
            .update(NodePatch::Task(TaskPatch {
                name: Some("x".into()),
                ..Default::default()
            }))
            .unwrap();
        assert_eq!(tree.get(&[0]).unwrap().name(), "x");
    }

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

    #[test]
    fn nearest_parent_container_from_container() {
        let t = tree();
        assert_eq!(t.nearest_file_owner(&[1]), Some(vec![1]));
    }

    #[test]
    fn nearest_parent_container_from_task() {
        let t = tree();
        assert_eq!(t.nearest_file_owner(&[1, 0]), Some(vec![1]));
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

    fn root_kind(t: &Tree) -> ContainerKind {
        match t.get(&[]) {
            Some(Node::Container(c)) => c.kind,
            _ => panic!("root is not a container"),
        }
    }

    #[tokio::test]
    async fn load_from_empty_dir_creates_root() {
        let tmp = tempfile::tempdir().unwrap();
        let root_dir = tmp.path().join("udo"); // does not exist yet

        let t = Tree::load_from(&root_dir).await.unwrap();

        assert_eq!(root_kind(&t), ContainerKind::Root);
        assert!(t.get(&[]).unwrap().children().is_empty());
        assert!(root_dir.join(crate::UDO_FILE_NAME).exists());
    }

    #[tokio::test]
    async fn load_roundtrip_nested() {
        let tmp = tempfile::tempdir().unwrap();
        let root_dir = tmp.path().to_path_buf();
        let ws_dir = root_dir.join("ws");
        std::fs::create_dir(&ws_dir).unwrap();

        // root -> ws (Workspace) -> task "t"
        let t = Tree {
            root: Node::Container(Container {
                name: "root".into(),
                dir: root_dir.clone(),
                kind: ContainerKind::Root,
                settings: ContainerSettings::default(),
                unloaded: vec![],
                collapsed: false,
                children: vec![Node::Container(Container {
                    name: "ws".into(),
                    dir: ws_dir.clone(),
                    kind: ContainerKind::Workspace,
                    settings: ContainerSettings::default(),
                    unloaded: vec![],
                    collapsed: false,
                    children: vec![task("t")],
                })],
            }),
            cursor: vec![],
        };
        t.save(&[]).await.unwrap(); // root file
        t.save(&[0]).await.unwrap(); // ws file

        let loaded = Tree::load_from(&root_dir).await.unwrap();

        assert_eq!(root_kind(&loaded), ContainerKind::Root);
        assert_eq!(loaded.get(&[]).unwrap().name(), "root");
        assert_eq!(loaded.get(&[0]).unwrap().name(), "ws");
        assert_eq!(loaded.get(&[0]).unwrap().dir(), Some(ws_dir.as_path()));
        assert_eq!(loaded.get(&[0, 0]).unwrap().name(), "t");
        assert!(loaded.get(&[1]).is_none());
    }

    #[tokio::test]
    async fn load_skips_missing_child() {
        let tmp = tempfile::tempdir().unwrap();
        let root_dir = tmp.path().to_path_buf();

        ContainerData {
            name: "root".into(),
            kind: ContainerKind::Root,
            tasks: vec![],
            children: vec![root_dir.join("gone")], // never created
            settings: ContainerSettings::default(),
        }
        .save(&root_dir)
        .await
        .unwrap();

        let loaded = Tree::load_from(&root_dir).await.unwrap();

        assert!(loaded.get(&[]).unwrap().children().is_empty());
    }

    // ---------- cursor fix-up (in memory, no disk) ----------

    /// tree() = root: [a, inner: [b]]. Remove `path` like `delete` does,
    /// without saving, and return the fixed-up cursor.
    fn cursor_after_remove(cursor: &[usize], path: &[usize]) -> NodePath {
        let mut t = tree();
        t.cursor = cursor.to_vec();
        let (&idx, parent) = path.split_last().unwrap();
        t.get_mut(parent)
            .unwrap()
            .children_mut()
            .unwrap()
            .remove(idx);
        t.fix_cursor_after_remove(parent, idx);
        t.cursor
    }

    #[test]
    fn cursor_on_later_sibling_shifts_left() {
        assert_eq!(cursor_after_remove(&[1], &[0]), vec![0]);
    }

    #[test]
    fn cursor_inside_later_sibling_shifts_left() {
        assert_eq!(cursor_after_remove(&[1, 0], &[0]), vec![0, 0]);
    }

    #[test]
    fn cursor_on_earlier_sibling_unchanged() {
        assert_eq!(cursor_after_remove(&[0], &[1]), vec![0]);
    }

    #[test]
    fn cursor_on_removed_takes_next_sibling() {
        assert_eq!(cursor_after_remove(&[0], &[0]), vec![0]); // now "inner"
    }

    #[test]
    fn cursor_on_removed_last_takes_previous_sibling() {
        assert_eq!(cursor_after_remove(&[1], &[1]), vec![0]); // now "a"
    }

    #[test]
    fn cursor_inside_removed_subtree_goes_to_sibling() {
        assert_eq!(cursor_after_remove(&[1, 0], &[1]), vec![0]);
    }

    #[test]
    fn cursor_on_removed_only_child_goes_to_parent() {
        assert_eq!(cursor_after_remove(&[1, 0], &[1, 0]), vec![1]);
    }

    #[test]
    fn cursor_on_root_unchanged() {
        assert_eq!(cursor_after_remove(&[], &[0]), Vec::<usize>::new());
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

    /// root (tmp) -> [task "a", ws (tmp/ws) -> [task "b"]], both files saved.
    async fn disk_tree(root_dir: &Path) -> Tree {
        let ws_dir = root_dir.join("ws");
        std::fs::create_dir(&ws_dir).unwrap();
        let t = Tree {
            root: Node::Container(Container {
                name: "root".into(),
                dir: root_dir.to_path_buf(),
                kind: ContainerKind::Root,
                settings: ContainerSettings::default(),
                unloaded: vec![],
                collapsed: false,
                children: vec![
                    task("a"),
                    Node::Container(Container {
                        name: "ws".into(),
                        dir: ws_dir,
                        kind: ContainerKind::Workspace,
                        settings: ContainerSettings::default(),
                        unloaded: vec![],
                        collapsed: false,
                        children: vec![task("b")],
                    }),
                ],
            }),
            cursor: vec![],
        };
        t.save(&[]).await.unwrap();
        t.save(&[1]).await.unwrap();
        t
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

    // ---------- name lookup (tree() = root: [a, inner: [b]]) ----------

    #[test]
    fn find_child_by_name() {
        let t = tree();
        assert_eq!(t.find_child(&[], "a"), Some(vec![0]));
        assert_eq!(t.find_child(&[], "inner"), Some(vec![1]));
        assert_eq!(t.find_child(&[1], "b"), Some(vec![1, 0]));
    }

    #[test]
    fn find_child_none_cases() {
        let t = tree();
        assert_eq!(t.find_child(&[], "nope"), None); // no such child
        assert_eq!(t.find_child(&[0], "x"), None); // parent is a task
        assert_eq!(t.find_child(&[9], "a"), None); // parent missing
        assert_eq!(t.find_child(&[], "b"), None); // only direct children
    }

    #[test]
    fn resolve_follows_names() {
        let t = tree();
        assert_eq!(t.resolve(&[]), Some(vec![]));
        assert_eq!(t.resolve(&["inner"]), Some(vec![1]));
        assert_eq!(t.resolve(&["inner", "b"]), Some(vec![1, 0]));
    }

    #[test]
    fn resolve_none_on_broken_chain() {
        let t = tree();
        assert_eq!(t.resolve(&["nope"]), None);
        assert_eq!(t.resolve(&["inner", "nope"]), None);
        assert_eq!(t.resolve(&["a", "x"]), None); // "a" is a task
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

    // ---------- create_task ----------

    fn new_task(name: &str, dir: Option<PathBuf>) -> Task {
        Task::new(name.into(), dir, Utc::now())
    }

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

    #[tokio::test]
    async fn save_errors_on_missing_path() {
        let t = tree(); // errors before writing, so /tmp is never touched
        assert!(t.save(&[9]).await.is_err());
        assert!(t.save(&[1, 5]).await.is_err());
    }

    #[test]
    fn nearest_file_owner_none_for_missing() {
        assert_eq!(tree().nearest_file_owner(&[9]), None);
    }

    // ---------- root dir ----------

    #[test]
    fn root_dir_uses_env_override() {
        let dir = root_dir_from(Some("/tmp/udo-test".into())).unwrap();
        assert_eq!(dir, PathBuf::from("/tmp/udo-test"));
    }

    #[test]
    fn root_dir_makes_relative_override_absolute() {
        let dir = root_dir_from(Some("udo-test".into())).unwrap();
        assert!(dir.is_absolute());
        assert!(dir.ends_with("udo-test"));
    }

    #[test]
    fn root_dir_defaults_to_config_when_unset_or_empty() {
        for env in [None, Some(OsString::new())] {
            let dir = root_dir_from(env).unwrap();
            assert!(dir.ends_with(".config/udo"));
        }
    }

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

    // ---------- name rules / default dirs ----------

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

    #[test]
    fn default_child_dir_is_parent_dir_plus_name() {
        let t = tree(); // root (/tmp/root): [a, inner (/tmp/inner): [b]]
        assert_eq!(
            t.default_child_dir(&[1], "new"),
            Some(PathBuf::from("/tmp/inner/new"))
        );
        assert_eq!(
            t.default_child_dir(&[], "new"),
            Some(PathBuf::from("/tmp/root/new"))
        );
        assert_eq!(t.default_child_dir(&[0], "new"), None); // task parent
        assert_eq!(t.default_child_dir(&[9], "new"), None); // missing
    }

    #[test]
    fn auto_task_dir_only_inside_projects() {
        let mut t = tree(); // "inner" is a Workspace
        assert_eq!(t.auto_task_dir(&[1], "c"), None);
        assert_eq!(t.auto_task_dir(&[], "c"), None); // root

        if let Some(Node::Container(c)) = t.get_mut(&[1]) {
            c.kind = ContainerKind::Project;
        }
        assert_eq!(
            t.auto_task_dir(&[1], "c"),
            Some(PathBuf::from("/tmp/inner/c"))
        );
        assert_eq!(t.auto_task_dir(&[0], "c"), None); // task parent
    }
}
