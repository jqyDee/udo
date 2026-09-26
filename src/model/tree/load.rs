use std::path::{Path, PathBuf};

use async_recursion::async_recursion;
use tokio::fs;

use crate::{
    Res, UDO_FILE_NAME,
    dir::root_dir,
    model::{
        container::{Container, ContainerKind},
        data::ContainerData,
        node::Node,
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

#[cfg(test)]
mod tests {
    use crate::model::{
        container::{Container, ContainerKind, ContainerSettings},
        data::ContainerData,
        node::Node,
        tree::{Tree, tests::task},
    };

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
}
