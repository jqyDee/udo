use serde::{Deserialize, Serialize};
use tokio::fs;

use crate::{
    Res, UDO_FILE_NAME,
    model::{
        container::ContainerKind,
        node::{Node, NodeBody, NodeHeader},
        settings::{ContainerSettings, RootSettings},
        task::Task,
    },
    persist::write_toml_atomic,
};
use std::path::{Path, PathBuf};

/// How a folder is written into the `.udo.toml` of the container at `own`:
/// relative if it lies inside `own` (it moves with it: another machine, a
/// git checkout), else absolute as it is. Never `..`, never empty.
fn to_file(own: &Path, dir: &Path) -> PathBuf {
    match dir.strip_prefix(own) {
        Ok(inside) if !inside.as_os_str().is_empty() => inside.to_path_buf(),
        _ => dir.to_path_buf(),
    }
}

/// A folder read from the `.udo.toml` of the container at `own`: relative
/// ones are below `own`, absolute ones stay as they are.
fn from_file(own: &Path, dir: PathBuf) -> PathBuf {
    if dir.is_relative() {
        own.join(dir)
    } else {
        dir
    }
}

/// One task row in the parent's `.udo.toml`. Flat on disk: header and
/// `Task` fields sit side by side.
#[derive(Serialize, Deserialize)]
pub struct TaskData {
    #[serde(flatten)]
    pub header: NodeHeader,
    #[serde(flatten)]
    pub task: Task,
}

impl TaskData {
    pub fn into_node(self) -> Node {
        Node {
            header: self.header,
            body: NodeBody::Task(self.task),
        }
    }
}

/// On-disk shape of one container's `.udo.toml`.
///
/// `tasks` stores the terminal children as rows; `children` stores only the
/// *dirs* of child containers (not their nested content — those live in their
/// own files). This is what keeps it one-file-per-container.
#[derive(Serialize, Deserialize)]
pub struct ContainerData {
    #[serde(flatten)]
    pub header: NodeHeader,
    pub kind: ContainerKind,
    #[serde(default)]
    pub tasks: Vec<TaskData>, // terminal children (task rows)
    #[serde(default)]
    pub children: Vec<PathBuf>, // child container dirs, relative if inside this one
    #[serde(flatten)]
    pub settings: ContainerSettings,
    #[serde(default, skip_serializing_if = "RootSettings::is_empty")]
    pub root: RootSettings,
}

impl ContainerData {
    /// Read <dir>/.udo.toml. Errors name the file, so a broken one is
    /// findable. Folders come back absolute (relative ones are below `dir`).
    pub async fn load(dir: &Path) -> Res<Self> {
        let path = dir.join(UDO_FILE_NAME);
        let content = fs::read_to_string(&path)
            .await
            .map_err(|e| format!("{}: {e}", path.display()))?;
        let mut data: Self =
            toml::from_str(&content).map_err(|e| format!("{}: {e}", path.display()))?;
        data.children = data
            .children
            .into_iter()
            .map(|child| from_file(dir, child))
            .collect();
        for row in &mut data.tasks {
            row.task.dir = row.task.dir.take().map(|d| from_file(dir, d));
        }
        data.settings.archive_dir = data.settings.archive_dir.take().map(|d| from_file(dir, d));
        Ok(data)
    }

    /// Write <dir>/.udo.toml atomically.
    pub async fn save(&self, dir: &Path) -> Res<()> {
        write_toml_atomic(&dir.join(UDO_FILE_NAME), self).await
    }
}

// DTO conversion for a container node. Fails for a task node. Folders are
// written in file form (`to_file`): relative if inside the container.
impl TryFrom<&Node> for ContainerData {
    type Error = &'static str;

    fn try_from(node: &Node) -> Result<Self, Self::Error> {
        let c = node.as_container().ok_or("file owner is not a container")?;
        let own = c.dir.as_path();
        Ok(ContainerData {
            header: node.header.clone(),
            kind: c.kind,
            tasks: c
                .children
                .iter()
                .filter_map(|n| match &n.body {
                    NodeBody::Task(t) => Some(TaskData {
                        header: n.header.clone(),
                        task: Task {
                            dir: t.dir.as_deref().map(|d| to_file(own, d)),
                            ..t.clone()
                        },
                    }),
                    NodeBody::Container(_) => None,
                })
                .collect(),
            // loaded children + ones we couldn't load (must not be dropped)
            children: c
                .container_children_paths()
                .into_iter()
                .chain(c.unloaded.iter().cloned())
                .map(|child| to_file(own, &child))
                .collect(),
            settings: ContainerSettings {
                archive_dir: c.settings.archive_dir.as_deref().map(|d| to_file(own, d)),
                ..c.settings.clone()
            },
            root: c.root_settings.clone(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        model::{
            container::{Container, ContainerKind},
            tree::Tree,
        },
        test_util::{container, container_at, task},
    };

    #[tokio::test]
    async fn container_data_roundtrips() {
        let dir = tempfile::tempdir().unwrap();
        let data = ContainerData {
            header: NodeHeader::new("w".into()),
            kind: ContainerKind::Workspace,
            tasks: vec![],
            children: vec![PathBuf::from("/tmp/sub")],
            settings: ContainerSettings {
                archive_dir: Some(PathBuf::from("/tmp/arch")),
                ..Default::default()
            },
            root: RootSettings::default(),
        };
        data.save(dir.path()).await.unwrap();

        let loaded = ContainerData::load(dir.path()).await.unwrap();
        assert_eq!(loaded.header.name, "w");
        assert_eq!(loaded.children, vec![PathBuf::from("/tmp/sub")]);
        // #[serde(flatten)] settings must survive the round-trip
        assert_eq!(
            loaded.settings.archive_dir,
            Some(PathBuf::from("/tmp/arch"))
        );
    }

    #[tokio::test]
    async fn load_error_names_the_file() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(UDO_FILE_NAME), "nope = 1").unwrap();

        let err = ContainerData::load(dir.path())
            .await
            .err()
            .unwrap()
            .to_string();

        assert!(err.contains(&dir.path().join(UDO_FILE_NAME).display().to_string()));
    }

    #[tokio::test]
    async fn load_missing_file_names_the_file() {
        let dir = tempfile::tempdir().unwrap();

        let err = ContainerData::load(dir.path())
            .await
            .err()
            .unwrap()
            .to_string();

        assert!(err.contains(UDO_FILE_NAME));
    }

    #[tokio::test]
    async fn headers_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let mut node = container("w", vec![task("t")]);
        node.header.description = Some("line one\nline two".into());
        let data = ContainerData::try_from(&node).unwrap();
        data.save(dir.path()).await.unwrap();

        let loaded = ContainerData::load(dir.path()).await.unwrap();

        // whole header: id, name, created_at, description
        assert_eq!(loaded.header, node.header);
        assert_eq!(loaded.tasks[0].header, node.children()[0].header);
    }

    #[test]
    fn try_from_keeps_task_rows_only() {
        let node = container("w", vec![container("inner", vec![]), task("t")]);
        let data = ContainerData::try_from(&node).unwrap();
        assert_eq!(data.header.name, "w");
        let names: Vec<_> = data.tasks.iter().map(|t| t.header.name.as_str()).collect();
        assert_eq!(names, vec!["t"]);
        assert_eq!(data.children, vec![PathBuf::from("/tmp/inner")]);
    }

    #[test]
    fn try_from_rejects_task_node() {
        assert!(ContainerData::try_from(&task("t")).is_err());
    }

    #[test]
    fn file_is_flat_and_skips_empty_description() {
        let node = container("w", vec![task("t")]);
        let text = toml::to_string(&ContainerData::try_from(&node).unwrap()).unwrap();
        assert!(text.starts_with("id = "), "got:\n{text}"); // header fields at the top
        assert!(text.contains("created_at = "));
        assert!(text.contains("[[tasks]]"));
        // no nested tables from `flatten`
        assert!(!text.contains("[header]") && !text.contains("[tasks.header]"));
        assert!(!text.contains("[tasks.task]"));
        assert!(!text.contains("description")); // None -> no line
    }

    // ---------- [root] table ----------

    #[tokio::test]
    async fn root_table_roundtrips() {
        let dir = tempfile::tempdir().unwrap();
        let data = ContainerData {
            header: NodeHeader::new("root".into()),
            kind: ContainerKind::Root,
            tasks: vec![],
            children: vec![],
            settings: ContainerSettings {
                archive_dir: Some(PathBuf::from("/arch")),
                ..Default::default()
            },
            root: RootSettings {
                theme: Some("dark".into()),
                ..Default::default()
            },
        };
        data.save(dir.path()).await.unwrap();

        let text = std::fs::read_to_string(dir.path().join(UDO_FILE_NAME)).unwrap();
        let loaded = ContainerData::load(dir.path()).await.unwrap();

        assert_eq!(loaded.root, data.root);
        // own table, not flattened like `settings`
        let (flat, table) = text.split_once("[root]").expect("no [root] table");
        assert!(table.contains("theme = \"dark\""), "got:\n{text}");
        assert!(!flat.contains("theme"), "theme outside [root]:\n{text}");
        assert_eq!(loaded.settings.archive_dir, Some(PathBuf::from("/arch")));
    }

    #[test]
    fn empty_root_settings_leave_no_table() {
        let node = container("w", vec![task("t")]);
        let text = toml::to_string(&ContainerData::try_from(&node).unwrap()).unwrap();
        assert!(!text.contains("[root]"), "got:\n{text}");
    }

    #[test]
    fn try_from_takes_the_root_settings_along() {
        let mut node = container("root", vec![]);
        let c = node.as_container_mut().unwrap();
        c.root_settings.theme = Some("dark".into());

        let data = ContainerData::try_from(&node).unwrap();

        assert_eq!(data.root.theme.as_deref(), Some("dark"));
    }

    // ---------- relative folders ----------

    /// A task with its own folder, due now.
    fn task_in(name: &str, dir: PathBuf) -> Node {
        Node::task(name.into(), Task::new(Some(dir), crate::model::time::now()))
    }

    /// A workspace at `tmp` with a task folder, a child container and an
    /// archive inside it, plus a child container outside it.
    fn node_with_folders(tmp: &Path, outside: &Path) -> Node {
        let mut c = Container::new(tmp.to_path_buf(), ContainerKind::Workspace);
        c.settings.archive_dir = Some(tmp.join("archive"));
        c.children = vec![
            task_in("lab 3", tmp.join("lab_3")),
            container_at("cs", &tmp.join("cs"), ContainerKind::Project, vec![]),
            container_at("far", outside, ContainerKind::Project, vec![]),
        ];
        Node::container("uni".into(), c)
    }

    /// The file as written, without `load`'s resolving.
    fn raw(dir: &Path) -> toml::Value {
        toml::from_str(&std::fs::read_to_string(dir.join(UDO_FILE_NAME)).unwrap()).unwrap()
    }

    #[tokio::test]
    async fn folders_inside_are_written_relative_and_outside_absolute() {
        let tmp = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let node = node_with_folders(tmp.path(), outside.path());

        ContainerData::try_from(&node)
            .unwrap()
            .save(tmp.path())
            .await
            .unwrap();

        let file = raw(tmp.path());
        let children: Vec<&str> = file["children"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert_eq!(children, vec!["cs", outside.path().to_str().unwrap()]);
        assert_eq!(file["tasks"][0]["dir"].as_str(), Some("lab_3"));
        assert_eq!(file["archive_dir"].as_str(), Some("archive"));
    }

    #[tokio::test]
    async fn load_gives_absolute_folders_again() {
        let tmp = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let node = node_with_folders(tmp.path(), outside.path());
        ContainerData::try_from(&node)
            .unwrap()
            .save(tmp.path())
            .await
            .unwrap();

        let data = ContainerData::load(tmp.path()).await.unwrap();

        assert_eq!(
            data.children,
            vec![tmp.path().join("cs"), outside.path().to_path_buf()]
        );
        assert_eq!(data.tasks[0].task.dir, Some(tmp.path().join("lab_3")));
        assert_eq!(data.settings.archive_dir, Some(tmp.path().join("archive")));
    }

    /// Files from before relative folders: absolute paths load as they are.
    #[tokio::test]
    async fn old_absolute_paths_still_load() {
        let tmp = tempfile::tempdir().unwrap();
        let cs = tmp.path().join("cs");
        let mut old = ContainerData::try_from(&container("uni", vec![])).unwrap();
        old.children = vec![cs.clone()];
        let text = toml::to_string(&old).unwrap(); // written as is: absolute
        std::fs::write(tmp.path().join(UDO_FILE_NAME), text).unwrap();

        let data = ContainerData::load(tmp.path()).await.unwrap();

        assert_eq!(data.children, vec![cs]);
    }

    /// A task using its container's own folder: absolute, never "".
    #[test]
    fn the_containers_own_folder_stays_absolute() {
        let tmp = tempfile::tempdir().unwrap();
        let mut c = Container::new(tmp.path().to_path_buf(), ContainerKind::Workspace);
        c.children = vec![task_in("sheet", tmp.path().to_path_buf())];

        let data = ContainerData::try_from(&Node::container("uni".into(), c)).unwrap();

        assert_eq!(data.tasks[0].task.dir, Some(tmp.path().to_path_buf()));
    }

    /// `/x/cs2` only shares text with `/x/cs`: not inside it.
    #[test]
    fn a_shared_text_prefix_is_not_inside() {
        let cs = Path::new("/x/cs");

        assert_eq!(to_file(cs, Path::new("/x/cs2")), PathBuf::from("/x/cs2"));
        assert_eq!(to_file(cs, Path::new("/x/cs/a")), PathBuf::from("a"));
    }

    /// An unloaded child (its folder is missing) keeps its entry, relative
    /// if inside.
    #[test]
    fn unloaded_children_are_kept_relative() {
        let mut node = container("uni", vec![]); // at /tmp/uni
        node.as_container_mut()
            .unwrap()
            .unloaded
            .push(PathBuf::from("/tmp/uni/gone"));

        let data = ContainerData::try_from(&node).unwrap();

        assert_eq!(data.children, vec![PathBuf::from("gone")]);
    }

    /// Moving the whole root folder (its workspaces inside it) keeps it
    /// loadable: everything inside is relative.
    #[tokio::test]
    async fn a_moved_root_loads_from_its_new_place() {
        let tmp = tempfile::tempdir().unwrap();
        let (old, new) = (tmp.path().join("old"), tmp.path().join("new"));
        std::fs::create_dir_all(old.join("ws")).unwrap();
        let tree = Tree::new(container_at(
            "root",
            &old,
            ContainerKind::Root,
            vec![container_at(
                "ws",
                &old.join("ws"),
                ContainerKind::Workspace,
                vec![task("b")],
            )],
        ));
        tree.save(&[]).await.unwrap();
        tree.save(&[0]).await.unwrap();

        std::fs::rename(&old, &new).unwrap();
        let moved = Tree::load_from(&new).await.unwrap();

        assert_eq!(
            moved.get(&[0]).unwrap().dir(),
            Some(new.join("ws").as_path())
        );
        assert_eq!(moved.get(&[0, 0]).unwrap().name(), "b");
    }
}
