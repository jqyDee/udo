use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

use async_recursion::async_recursion;
use tokio::fs;

use crate::{
    Res, UDO_FILE_NAME,
    dir::root_dir,
    model::{
        NodePath,
        container::{Container, ContainerKind},
        data::{ContainerData, TaskData},
        id::NodeId,
        node::{Node, NodeBody},
        tree::Tree,
        view::ViewState,
    },
};

impl Tree {
    /// Load the tree from the on-disk file structure.
    pub async fn load() -> Res<Self> {
        Self::load_from(&root_dir()?).await
    }

    pub async fn load_from(root_dir: &Path) -> Res<Self> {
        if !root_dir.join(UDO_FILE_NAME).exists() {
            println!("First run: creating root in {root_dir:?}!");
            fs::create_dir_all(root_dir).await?;

            let root = Container::new(root_dir.to_path_buf(), ContainerKind::Root);
            let tree = Self::new(Node::container("root".into(), root));
            tree.save(&[]).await?;
            return Ok(tree);
        }
        let mut loaded = HashSet::from([root_dir.to_path_buf()]);
        let root = Self::build_container(root_dir.to_path_buf(), &mut loaded).await?;
        let mut tree = Self::new(root);

        // Write back files whose ids changed, so the new ones stay stable.
        let changed = tree.fix_duplicate_ids();
        let mut owners: Vec<NodePath> = changed
            .iter()
            .filter_map(|p| tree.nearest_file_owner(p))
            .collect();
        owners.sort();
        owners.dedup();
        for owner in &owners {
            tree.save(owner).await?;
        }

        tree.apply_view(&ViewState::load(root_dir).await);
        Ok(tree)
    }

    /// Recursively build one container from its dir: load DTO, map task rows
    /// -> task nodes, recurse each child dir -> container node.
    ///
    /// `loaded` holds every dir built so far, so a dir listed by two parents
    /// is only loaded once.
    ///
    /// `#[async_recursion]` boxes the future (async fn can't recurse, E0733).
    #[async_recursion]
    async fn build_container(dir: PathBuf, loaded: &mut HashSet<PathBuf>) -> Res<Node> {
        let data = ContainerData::load(&dir).await?;

        let mut children: Vec<Node> = data.tasks.into_iter().map(TaskData::into_node).collect();
        let mut unloaded = vec![];

        for child_path in data.children {
            if !child_path.join(UDO_FILE_NAME).exists() {
                eprintln!("warning: could not find a container in location {child_path:?}");
                unloaded.push(child_path); // keep it registered, see Container::unloaded
                continue;
            }
            // e.g. after `cp -r`: the copy's file still lists the original's children
            if !loaded.insert(child_path.clone()) {
                eprintln!(
                    "warning: {child_path:?} is listed by more than one container, loaded only once"
                );
                unloaded.push(child_path);
                continue;
            }

            children.push(Self::build_container(child_path, loaded).await?);
        }

        Ok(Node {
            id: data.id,
            name: data.name,
            body: NodeBody::Container(Container {
                dir,
                kind: data.kind,
                settings: data.settings,
                children,
                unloaded,
                collapsed: false,
            }),
        })
    }

    /// Give every node whose id was already seen a fresh one (first in
    /// depth-first order keeps it). Returns the paths of changed nodes.
    fn fix_duplicate_ids(&mut self) -> Vec<NodePath> {
        let mut seen = HashSet::new();
        let mut changed = vec![];
        fix_ids(&mut self.root, &mut vec![], &mut seen, &mut changed);
        changed
    }
}

fn fix_ids(
    node: &mut Node,
    path: &mut NodePath,
    seen: &mut HashSet<NodeId>,
    changed: &mut Vec<NodePath>,
) {
    if !seen.insert(node.id) {
        let fresh = NodeId::new();
        eprintln!("warning: duplicate id {} ({}), new id {fresh}", node.id, node.name);
        node.id = fresh;
        seen.insert(fresh);
        changed.push(path.clone());
    }
    if let Some(children) = node.children_mut() {
        for (i, child) in children.iter_mut().enumerate() {
            path.push(i);
            fix_ids(child, path, seen, changed);
            path.pop();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::HashSet, path::PathBuf};

    use chrono::Utc;

    use crate::{
        model::{
            container::{ContainerKind, ContainerSettings},
            data::{ContainerData, TaskData},
            id::NodeId,
            node::Node,
            task::Task,
            tree::Tree,
        },
        test_util::{container_at, task},
    };

    fn root_kind(t: &Tree) -> ContainerKind {
        let root = t.get(&[]).and_then(Node::as_container);
        root.expect("root is not a container").kind
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
        let t = Tree::new(container_at(
            "root",
            &root_dir,
            ContainerKind::Root,
            vec![container_at("ws", &ws_dir, ContainerKind::Workspace, vec![task("t")])],
        ));
        t.save(&[]).await.unwrap(); // root file
        t.save(&[0]).await.unwrap(); // ws file

        let loaded = Tree::load_from(&root_dir).await.unwrap();

        assert_eq!(root_kind(&loaded), ContainerKind::Root);
        assert_eq!(loaded.get(&[]).unwrap().name(), "root");
        assert_eq!(loaded.get(&[0]).unwrap().name(), "ws");
        assert_eq!(loaded.get(&[0]).unwrap().dir(), Some(ws_dir.as_path()));
        assert_eq!(loaded.get(&[0, 0]).unwrap().name(), "t");
        assert!(loaded.get(&[1]).is_none());
        assert_eq!(ids(&loaded), ids(&t));
    }

    /// Every id in the tree, depth-first.
    fn ids(t: &Tree) -> Vec<NodeId> {
        fn walk(n: &Node, out: &mut Vec<NodeId>) {
            out.push(n.id());
            n.children().iter().for_each(|c| walk(c, out));
        }
        let mut out = vec![];
        walk(t.get(&[]).unwrap(), &mut out);
        out
    }

    fn container_data(name: &str, tasks: Vec<TaskData>, children: Vec<PathBuf>) -> ContainerData {
        ContainerData {
            id: NodeId::new(),
            name: name.into(),
            kind: ContainerKind::Workspace,
            tasks,
            children,
            settings: ContainerSettings::default(),
        }
    }

    #[tokio::test]
    async fn load_copied_container_gets_fresh_ids_that_stay_stable() {
        let tmp = tempfile::tempdir().unwrap();
        let root_dir = tmp.path().to_path_buf();
        let (a, b) = (root_dir.join("a"), root_dir.join("b"));
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        let mut root = container_data("root", vec![], vec![a.clone(), b.clone()]);
        root.kind = ContainerKind::Root;
        root.save(&root_dir).await.unwrap();
        // `cp -r a b`: same container id and same task id in both files
        let row = TaskData {
            id: NodeId::new(),
            name: "t".into(),
            task: Task::new(None, Utc::now()),
        };
        let copied = container_data("ws", vec![row], vec![]);
        copied.save(&a).await.unwrap();
        copied.save(&b).await.unwrap();

        let first = Tree::load_from(&root_dir).await.unwrap();
        let second = Tree::load_from(&root_dir).await.unwrap();

        let all = ids(&first);
        let unique: HashSet<_> = all.iter().collect();
        assert_eq!(all.len(), 5);
        assert_eq!(unique.len(), 5);
        assert_eq!(first.get(&[0]).unwrap().id(), copied.id); // first one keeps it
        assert_eq!(all, ids(&second));
    }

    #[tokio::test]
    async fn load_dir_listed_twice_is_loaded_once() {
        let tmp = tempfile::tempdir().unwrap();
        let root_dir = tmp.path().to_path_buf();
        let (a, b, shared) = (root_dir.join("a"), root_dir.join("b"), root_dir.join("s"));
        for d in [&a, &b, &shared] {
            std::fs::create_dir_all(d).unwrap();
        }
        let mut root = container_data("root", vec![], vec![a.clone(), b.clone()]);
        root.kind = ContainerKind::Root;
        root.save(&root_dir).await.unwrap();
        container_data("a", vec![], vec![shared.clone()]).save(&a).await.unwrap();
        container_data("b", vec![], vec![shared.clone()]).save(&b).await.unwrap();
        container_data("s", vec![], vec![]).save(&shared).await.unwrap();

        let first = Tree::load_from(&root_dir).await.unwrap();
        let second = Tree::load_from(&root_dir).await.unwrap();

        assert_eq!(first.get(&[0, 0]).unwrap().name(), "s");
        assert!(first.get(&[1]).unwrap().children().is_empty());
        assert_eq!(ids(&first), ids(&second));
        // still registered in b's file, not dropped
        let b_data = ContainerData::load(&b).await.unwrap();
        assert_eq!(b_data.children, vec![shared]);
    }

    #[tokio::test]
    async fn load_skips_missing_child() {
        let tmp = tempfile::tempdir().unwrap();
        let root_dir = tmp.path().to_path_buf();

        ContainerData {
            id: NodeId::new(),
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
}
