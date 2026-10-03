//! `udo add project|workspace NODE [--dir] [--description] [--no-run]`.

use std::path::{Path, PathBuf, absolute};

use super::{Added, RunFlag};
use crate::{
    Res,
    cli::resolve::{path_text, resolve_parent},
    core::Core,
    model::{
        NodePath,
        container::{Container, ContainerKind},
        node::Node,
    },
};

#[derive(clap::Args)]
pub struct AddContainerArgs {
    /// Path of the new workspace / project
    pub node: String,
    /// Folder (relative to the current folder); default: below the parent's
    #[arg(long)]
    pub dir: Option<PathBuf>,
    #[arg(long)]
    pub description: Option<String>,
    #[command(flatten)]
    pub run: RunFlag,
}

/// Add a workspace or project: its path (for `on_create`, run by the
/// caller) and the report. `args.run` is the caller's too.
pub async fn run(
    core: &mut Core,
    cwd: &Path,
    args: &AddContainerArgs,
    kind: ContainerKind,
) -> Res<(NodePath, Added)> {
    let (parent, name) = resolve_parent(core.tree(), &args.node, cwd)?;
    let dir = match &args.dir {
        Some(dir) => absolute(cwd.join(dir))?,
        None => core
            .container_dir(&parent, &name)
            .ok_or("the parent has no folder, give --dir")?,
    };
    let node = Node::container(name, Container::new(dir.clone(), kind))
        .with_description(args.description.clone());
    let path = core.create(&parent, node).await?;
    let added = Added {
        what: kind.to_string(),
        path: path_text(core.tree(), &path),
        dir: Some(dir),
        ran: None,
    };
    Ok((path, added))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::core; // disk_tree, root: [a, ws: [b]]

    #[tokio::test]
    async fn a_project_gets_a_folder_below_its_parent() {
        let (tmp, mut core) = core().await;
        let args = AddContainerArgs {
            node: "ws/cs 101".into(),
            dir: None,
            description: None,
            run: RunFlag { no_run: false },
        };

        let (_, added) = run(&mut core, tmp.path(), &args, ContainerKind::Project)
            .await
            .unwrap();

        assert_eq!(added.path, "ws/cs 101");
        assert_eq!(added.what, "project");
        assert!(tmp.path().join("ws").join("cs_101").is_dir());
    }
}
