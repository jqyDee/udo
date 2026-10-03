//! What a script learns about why and where it runs: `RunContext`, handed
//! over as `UDO_*` environment variables (no template language).

use std::{
    ffi::OsString,
    path::{Path, PathBuf},
};

use crate::model::{id::NodeId, node::Node, tree::Tree};

/// Why a script runs (`UDO_EVENT`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    /// `o`, `udo run`: `open_with`.
    Open,
    /// After creating a node: `on_create`.
    Create,
}

impl Event {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Create => "create",
        }
    }
}

/// Task or container (`UDO_NODE_KIND`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeKind {
    Task,
    Container,
}

impl NodeKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Task => "task",
            Self::Container => "container",
        }
    }
}

/// The opened / created node.
#[derive(Debug, Clone, PartialEq)]
pub struct NodeInfo {
    pub id: NodeId,
    pub name: String,
    pub kind: NodeKind,
    /// A task without a folder: None.
    pub dir: Option<PathBuf>,
}

/// The task the time goes to: the node itself, or the one picked for a
/// container.
#[derive(Debug, Clone, PartialEq)]
pub struct TaskInfo {
    pub id: NodeId,
    pub name: String,
    pub dir: Option<PathBuf>,
}

/// Everything a script learns about why and where it runs.
#[derive(Debug, Clone, PartialEq)]
pub struct RunContext {
    pub event: Event,
    pub node: NodeInfo,
    /// None: no task (e.g. `create` of a container).
    pub task: Option<TaskInfo>,
    /// The nearest container folder (a container: its own).
    pub container_dir: PathBuf,
    pub root: PathBuf,
}

/// The `UDO_TASK_*` variables: left out without a task, and removed from
/// what udo itself inherited (`launch`).
pub(super) const TASK_VARS: [&str; 3] = ["UDO_TASK_ID", "UDO_TASK_NAME", "UDO_TASK_DIR"];

impl RunContext {
    /// The context for `node` in `tree`; `task`: the task the time goes to
    /// (`Some(node)` when opening a task). None: a path is missing or
    /// `task` is no task.
    pub fn new(tree: &Tree, event: Event, node: &[usize], task: Option<&[usize]>) -> Option<Self> {
        let container = tree.nearest_file_owner(node)?;
        let task = match task {
            Some(path) => Some(task_info(tree.get(path)?)?),
            None => None,
        };
        Some(Self {
            event,
            node: node_info(tree.get(node)?),
            task,
            container_dir: tree.get(&container)?.dir()?.to_path_buf(),
            root: tree.root.dir()?.to_path_buf(),
        })
    }

    /// Where the script runs: the node's folder, else its container's (the
    /// root is a container and always has one).
    pub fn working_dir(&self) -> &Path {
        self.node.dir.as_deref().unwrap_or(&self.container_dir)
    }

    /// The `UDO_*` variables, in a fixed order. A missing folder is an
    /// empty value; without a task the `UDO_TASK_*` ones are left out.
    /// `bin`: the running udo (`UDO_BIN`, for hooks a script installs).
    pub fn env(&self, bin: &Path) -> Vec<(&'static str, OsString)> {
        let dir = |d: &Option<PathBuf>| d.clone().map(OsString::from).unwrap_or_default();
        let mut vars = vec![
            ("UDO_EVENT", self.event.as_str().into()),
            ("UDO_NODE_ID", self.node.id.to_string().into()),
            ("UDO_NODE_NAME", self.node.name.clone().into()),
            ("UDO_NODE_KIND", self.node.kind.as_str().into()),
            ("UDO_NODE_DIR", dir(&self.node.dir)),
        ];
        if let Some(task) = &self.task {
            let [id, name, task_dir] = TASK_VARS;
            vars.push((id, task.id.to_string().into()));
            vars.push((name, task.name.clone().into()));
            vars.push((task_dir, dir(&task.dir)));
        }
        vars.push(("UDO_CONTAINER_DIR", self.container_dir.clone().into()));
        vars.push(("UDO_ROOT", self.root.clone().into()));
        vars.push(("UDO_BIN", bin.into()));
        vars
    }
}

fn node_info(node: &Node) -> NodeInfo {
    NodeInfo {
        id: node.id(),
        name: node.name().into(),
        kind: match node.as_task() {
            Some(_) => NodeKind::Task,
            None => NodeKind::Container,
        },
        dir: node.dir().map(Path::to_path_buf),
    }
}

/// None: `node` is a container.
fn task_info(node: &Node) -> Option<TaskInfo> {
    node.as_task()?;
    Some(TaskInfo {
        id: node.id(),
        name: node.name().into(),
        dir: node.dir().map(Path::to_path_buf),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::{container, new_task, tree_with};

    /// root (/tmp/root): [notes (task), uni (/tmp/uni): [lab 3 (task,
    /// /tmp/uni/lab_3), sheet (task, no folder)]]
    fn tree() -> Tree {
        let lab = new_task("lab 3", Some(PathBuf::from("/tmp/uni/lab_3")));
        let uni = container("uni", vec![lab, new_task("sheet", None)]);
        tree_with(vec![new_task("notes", None), uni])
    }

    /// The variables as text, for comparing.
    fn vars(ctx: &RunContext) -> Vec<(&'static str, String)> {
        ctx.env(Path::new("/bin/udo"))
            .into_iter()
            .map(|(k, v)| (k, v.into_string().unwrap()))
            .collect()
    }

    fn var<'a>(vars: &'a [(&str, String)], key: &str) -> Option<&'a str> {
        vars.iter()
            .find(|(k, _)| *k == key)
            .map(|(_, v)| v.as_str())
    }

    #[test]
    fn opening_a_task_the_task_is_the_node() {
        let t = tree();
        let ctx = RunContext::new(&t, Event::Open, &[1, 0], Some(&[1, 0])).unwrap();
        let id = t.get(&[1, 0]).unwrap().id().to_string();

        assert_eq!(
            vars(&ctx),
            [
                ("UDO_EVENT", "open".to_string()),
                ("UDO_NODE_ID", id.clone()),
                ("UDO_NODE_NAME", "lab 3".into()),
                ("UDO_NODE_KIND", "task".into()),
                ("UDO_NODE_DIR", "/tmp/uni/lab_3".into()),
                ("UDO_TASK_ID", id),
                ("UDO_TASK_NAME", "lab 3".into()),
                ("UDO_TASK_DIR", "/tmp/uni/lab_3".into()),
                ("UDO_CONTAINER_DIR", "/tmp/uni".into()),
                ("UDO_ROOT", "/tmp/root".into()),
                ("UDO_BIN", "/bin/udo".into()),
            ]
        );
        assert_eq!(ctx.working_dir(), Path::new("/tmp/uni/lab_3"));
    }

    #[test]
    fn opening_a_container_the_task_is_the_picked_one() {
        let t = tree();
        let ctx = RunContext::new(&t, Event::Open, &[1], Some(&[1, 1])).unwrap();
        let vars = vars(&ctx);

        assert_eq!(var(&vars, "UDO_NODE_KIND"), Some("container"));
        assert_eq!(var(&vars, "UDO_NODE_DIR"), Some("/tmp/uni"));
        assert_eq!(var(&vars, "UDO_TASK_NAME"), Some("sheet"));
        assert_eq!(var(&vars, "UDO_TASK_DIR"), Some("")); // no folder
        assert_eq!(var(&vars, "UDO_CONTAINER_DIR"), Some("/tmp/uni")); // its own
        assert_eq!(ctx.working_dir(), Path::new("/tmp/uni"));
    }

    #[test]
    fn creating_a_container_has_no_task() {
        let ctx = RunContext::new(&tree(), Event::Create, &[1], None).unwrap();
        let vars = vars(&ctx);

        assert_eq!(var(&vars, "UDO_EVENT"), Some("create"));
        assert!(
            vars.iter().all(|(k, _)| !k.starts_with("UDO_TASK_")),
            "{vars:?}"
        );
    }

    /// No folder of its own: the script runs in the container's.
    #[test]
    fn a_task_without_a_folder_runs_in_its_containers() {
        let ctx = RunContext::new(&tree(), Event::Open, &[0], Some(&[0])).unwrap();

        assert_eq!(var(&vars(&ctx), "UDO_NODE_DIR"), Some(""));
        assert_eq!(ctx.working_dir(), Path::new("/tmp/root")); // the root's
    }

    #[test]
    fn a_missing_path_or_a_container_as_task_is_none() {
        let t = tree();

        assert!(RunContext::new(&t, Event::Open, &[7], None).is_none());
        assert!(RunContext::new(&t, Event::Open, &[1], Some(&[7])).is_none());
        assert!(RunContext::new(&t, Event::Open, &[1], Some(&[1])).is_none());
    }
}
