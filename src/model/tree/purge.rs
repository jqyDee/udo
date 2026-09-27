//! Full delete: a node together with its folders. `purge_plan` computes and
//! checks everything, the TUI shows the plan, `purge` executes exactly it.

use std::path::{Path, PathBuf};

use directories::BaseDirs;

use crate::{
    Res,
    model::{NodePath, id::NodeId, node::Node, tree::Tree},
};

/// What a full delete of one node would do. Built by `Tree::purge_plan`,
/// shown in the popup, executed unchanged by `Tree::purge`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PurgePlan {
    /// The node, to check it is still the same one when executing.
    pub path: NodePath,
    pub id: NodeId,
    pub name: String,
    /// The node's own folder, exactly as shown; the text to type.
    pub dir: PathBuf,
    /// Every folder that goes to the Trash, top folders only: a folder
    /// inside another listed folder is not listed again.
    pub folders: Vec<PathBuf>,
    /// Those of `folders` that are not inside `dir`, listed separately.
    pub outside: Vec<PathBuf>,
    /// Nodes below the node, not counting itself.
    pub containers: usize,
    pub tasks: usize,
}

/// Moves one folder away. Production: the system Trash; tests: a fake.
pub type TrashFn = fn(&Path) -> Result<(), String>;

/// The production `TrashFn`: the system Trash.
pub fn system_trash(p: &Path) -> Result<(), String> {
    trash::delete(p).map_err(|e| e.to_string())
}

/// What happened to the folders. The node is gone from udo either way.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct PurgeReport {
    pub trashed: Vec<PathBuf>,
    /// Folders that could not be trashed, with the reason.
    pub failed: Vec<(PathBuf, String)>,
}

impl Tree {
    /// Execute `plan`: check it is still current, unregister the node, then
    /// trash its folders. Unregistering first keeps udo consistent if
    /// trashing fails halfway: the report names what is still on disk.
    pub async fn purge(&mut self, plan: &PurgePlan, trash: TrashFn) -> Res<PurgeReport> {
        let dirs = BaseDirs::new().ok_or("could not find the home dir")?;
        self.purge_in(plan, trash, dirs.home_dir()).await
    }

    /// `purge` with the home dir passed in (see `purge_plan_in`).
    async fn purge_in(
        &mut self,
        plan: &PurgePlan,
        trash: TrashFn,
        home: &Path,
    ) -> Res<PurgeReport> {
        let fresh = self.purge_plan_in(&plan.path, home)?;
        if !fresh.is_some_and(|f| f.id == plan.id && f.folders == plan.folders) {
            return Err(format!("{} changed, open the delete again", plan.name).into());
        }

        self.delete(&plan.path).await?; // fails -> nothing trashed

        let mut report = PurgeReport::default();
        for folder in &plan.folders {
            let owned = folder.clone();
            // the crate blocks: keep it off the async threads
            let result = tokio::task::spawn_blocking(move || trash(&owned))
                .await
                .unwrap_or_else(|e| Err(e.to_string()));
            match result {
                Ok(()) => report.trashed.push(folder.clone()),
                Err(e) => report.failed.push((folder.clone(), e)),
            }
        }
        Ok(report)
    }

    /// `Ok(None)`: the node has no folder of its own, or none of its
    /// folders exists (nothing to delete on disk). `Err`: the path is the
    /// root or missing, or a safety check failed; the message names the
    /// folder and the reason.
    pub fn purge_plan(&self, path: &[usize]) -> Res<Option<PurgePlan>> {
        let dirs = BaseDirs::new().ok_or("could not find the home dir")?;
        self.purge_plan_in(path, dirs.home_dir())
    }

    /// `purge_plan` with the home dir passed in, so tests don't depend on
    /// who runs them.
    fn purge_plan_in(&self, path: &[usize], home: &Path) -> Res<Option<PurgePlan>> {
        if path.is_empty() {
            return Err("the root cannot be deleted".into());
        }
        let node = self.get(path).ok_or("no node at the path")?;
        let Some(dir) = node.dir() else {
            return Ok(None);
        };

        let mut found = vec![];
        collect_dirs(node, &mut found);
        found.retain(|d| d.exists());
        let folders = top_folders(found);
        if folders.is_empty() {
            return Ok(None);
        }
        self.check_folders(path, node.name(), &folders, home)?;

        let own = resolved(dir);
        let outside = folders
            .iter()
            .filter(|f| !resolved(f).starts_with(&own))
            .cloned()
            .collect();
        let (containers, tasks) = count_below(node);
        Ok(Some(PurgePlan {
            path: path.to_vec(),
            id: node.id(),
            name: node.name().to_string(),
            dir: dir.to_path_buf(),
            folders,
            outside,
            containers,
            tasks,
        }))
    }

    /// Error for the first folder that must not go: it is the udo root or
    /// above it, the home dir or above it, or it holds the folder of a node
    /// outside the subtree at `path`. `name` is that node's, for the message.
    fn check_folders(
        &self,
        path: &[usize],
        name: &str,
        folders: &[PathBuf],
        home: &Path,
    ) -> Res<()> {
        let root = self.root.dir();
        let home = resolved(home);
        let others = self.dirs_outside(path);

        for folder in folders {
            // stored OR resolved: a link inside `folder` goes with it even
            // when it points elsewhere
            let inside =
                |dir: &Path| dir.starts_with(folder) || resolved(dir).starts_with(resolved(folder));
            let why = if root.is_some_and(inside) {
                "contains the udo root folder".to_string()
            } else if home.starts_with(resolved(folder)) {
                "contains your home folder".to_string()
            } else if let Some((_, who)) = others.iter().find(|(d, _)| inside(d)) {
                format!("contains the folder of {who} (not part of {name})")
            } else {
                continue;
            };
            return Err(format!("refusing: {} {why}", folder.display()).into());
        }
        Ok(())
    }

    /// Dirs (as stored) of every node outside the subtree at `skip` (node
    /// dirs and `unloaded` dirs), each with who uses it. Like `dir_owner`,
    /// but callers check `starts_with`, not equality.
    fn dirs_outside(&self, skip: &[usize]) -> Vec<(PathBuf, String)> {
        fn walk(
            node: &Node,
            path: &mut NodePath,
            skip: &[usize],
            out: &mut Vec<(PathBuf, String)>,
        ) {
            if path.as_slice() == skip {
                return;
            }
            if let Some(dir) = node.dir() {
                out.push((dir.to_path_buf(), format!("{:?}", node.name())));
            }
            let Some(c) = node.as_container() else {
                return;
            };
            for u in &c.unloaded {
                out.push((
                    u.clone(),
                    format!("an unloaded container in {:?}", node.name()),
                ));
            }
            for (i, child) in c.children.iter().enumerate() {
                path.push(i);
                walk(child, path, skip, out);
                path.pop();
            }
        }
        let mut out = vec![];
        walk(&self.root, &mut vec![], skip, &mut out);
        out
    }
}

/// Every dir set in `node`'s subtree, `node` included: node dirs and
/// `unloaded` child dirs.
fn collect_dirs(node: &Node, out: &mut Vec<PathBuf>) {
    out.extend(node.dir().map(Path::to_path_buf));
    if let Some(c) = node.as_container() {
        out.extend(c.unloaded.iter().cloned());
        for child in &c.children {
            collect_dirs(child, out);
        }
    }
}

/// Only folders not inside (or equal to) another one, sorted by resolved
/// path; as stored.
fn top_folders(dirs: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut dirs: Vec<(PathBuf, PathBuf)> = dirs.into_iter().map(|d| (resolved(&d), d)).collect();
    // parents sort before their children, so one pass is enough
    dirs.sort();
    let mut top: Vec<(PathBuf, PathBuf)> = vec![];
    for (real, dir) in dirs {
        if !top.iter().any(|(t, _)| real.starts_with(t)) {
            top.push((real, dir));
        }
    }
    top.into_iter().map(|(_, dir)| dir).collect()
}

/// (containers, tasks) below `node`, not counting itself.
fn count_below(node: &Node) -> (usize, usize) {
    node.children().iter().fold((0, 0), |(c, t), child| {
        let (cc, ct) = count_below(child);
        if child.owns_file() {
            (c + 1 + cc, t + ct)
        } else {
            (c + cc, t + 1 + ct)
        }
    })
}

/// `path` with `..` and symlinks resolved as far as it exists on disk; a
/// missing rest is appended as stored. Only for comparing: the Trash gets
/// the stored path.
fn resolved(path: &Path) -> PathBuf {
    for base in path.ancestors() {
        if let Ok(real) = std::fs::canonicalize(base) {
            // `base` is an ancestor of `path`, so this never falls back
            return real.join(path.strip_prefix(base).unwrap_or(path));
        }
    }
    path.to_path_buf()
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use chrono::Utc;

    use crate::{
        Res,
        model::{container::ContainerKind, id::NodeId, node::Node, task::Task, tree::Tree},
        test_util::{container_at, task},
    };

    use super::{PurgePlan, PurgeReport, TrashFn};

    /// `tmp/<rel>`, created on disk.
    fn mk(tmp: &Path, rel: &str) -> PathBuf {
        let dir = tmp.join(rel);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn task_at(name: &str, dir: &Path) -> Node {
        Node::task(name.into(), Task::new(Some(dir.to_path_buf()), Utc::now()))
    }

    /// In memory, every dir created under `tmp`:
    ///
    /// ```text
    /// root (udo)
    /// ├─ [0] uni (uni)
    /// │  ├─ [0,0] algo (uni/algo, project)
    /// │  │  ├─ [0,0,0] "lab 3" (uni/algo/lab_3)
    /// │  │  └─ [0,0,1] notes (no dir)
    /// │  └─ [0,1] data (data: custom dir outside uni)
    /// ├─ [1] shared (shared)
    /// └─ [2] todo (no dir)
    /// ```
    fn fixture(tmp: &Path) -> Tree {
        let algo = container_at(
            "algo",
            &mk(tmp, "uni/algo"),
            ContainerKind::Project,
            vec![task_at("lab 3", &mk(tmp, "uni/algo/lab_3")), task("notes")],
        );
        let uni = container_at(
            "uni",
            &mk(tmp, "uni"),
            ContainerKind::Workspace,
            vec![algo, task_at("data", &mk(tmp, "data"))],
        );
        let shared = container_at(
            "shared",
            &mk(tmp, "shared"),
            ContainerKind::Workspace,
            vec![],
        );
        Tree::new(container_at(
            "root",
            &mk(tmp, "udo"),
            ContainerKind::Root,
            vec![uni, shared, task("todo")],
        ))
    }

    /// `purge_plan_in` with `tmp/home` (not created) as the home dir.
    fn plan(t: &Tree, tmp: &Path, path: &[usize]) -> Res<Option<PurgePlan>> {
        t.purge_plan_in(path, &tmp.join("home"))
    }

    // ---------- plan ----------

    #[test]
    fn task_with_folder() {
        let tmp = tempfile::tempdir().unwrap();
        let t = fixture(tmp.path());
        let lab = tmp.path().join("uni/algo/lab_3");

        let p = plan(&t, tmp.path(), &[0, 0, 0]).unwrap().unwrap();

        assert_eq!(p.path, vec![0, 0, 0]);
        assert_eq!(p.id, t.get(&[0, 0, 0]).unwrap().id());
        assert_eq!(p.name, "lab 3");
        assert_eq!(p.dir, lab);
        assert_eq!(p.folders, vec![lab]);
        assert!(p.outside.is_empty());
        assert_eq!((p.containers, p.tasks), (0, 0));
    }

    #[test]
    fn task_without_folder_is_none() {
        let tmp = tempfile::tempdir().unwrap();
        let t = fixture(tmp.path());
        assert_eq!(plan(&t, tmp.path(), &[2]).unwrap(), None);
        assert_eq!(plan(&t, tmp.path(), &[0, 0, 1]).unwrap(), None);
    }

    #[test]
    fn folder_missing_on_disk_is_none() {
        let tmp = tempfile::tempdir().unwrap();
        let t = fixture(tmp.path());
        std::fs::remove_dir(tmp.path().join("uni/algo/lab_3")).unwrap();
        assert_eq!(plan(&t, tmp.path(), &[0, 0, 0]).unwrap(), None);
    }

    #[test]
    fn container_collects_top_folders_outside_and_counts() {
        let tmp = tempfile::tempdir().unwrap();
        let t = fixture(tmp.path());
        let (uni, data) = (tmp.path().join("uni"), tmp.path().join("data"));

        let p = plan(&t, tmp.path(), &[0]).unwrap().unwrap();

        assert_eq!(p.dir, uni);
        // uni/algo and uni/algo/lab_3 are inside uni: only uni is listed
        assert_eq!(p.folders, vec![data.clone(), uni]);
        assert_eq!(p.outside, vec![data]);
        assert_eq!((p.containers, p.tasks), (1, 3)); // algo; lab 3, notes, data
    }

    #[test]
    fn missing_folders_are_skipped() {
        let tmp = tempfile::tempdir().unwrap();
        let t = fixture(tmp.path());
        std::fs::remove_dir(tmp.path().join("data")).unwrap();

        let p = plan(&t, tmp.path(), &[0]).unwrap().unwrap();

        assert_eq!(p.folders, vec![tmp.path().join("uni")]);
        assert!(p.outside.is_empty());
    }

    #[test]
    fn unloaded_dirs_are_included() {
        let tmp = tempfile::tempdir().unwrap();
        let mut t = fixture(tmp.path());
        let old = mk(tmp.path(), "old");
        let uni = t.get_mut(&[0]).and_then(Node::as_container_mut).unwrap();
        uni.unloaded.push(old.clone());

        let p = plan(&t, tmp.path(), &[0]).unwrap().unwrap();

        let data = tmp.path().join("data");
        assert_eq!(
            p.folders,
            vec![data.clone(), old.clone(), tmp.path().join("uni")]
        );
        assert_eq!(p.outside, vec![data, old]);
    }

    #[test]
    fn same_folder_twice_is_listed_once() {
        let tmp = tempfile::tempdir().unwrap();
        let mut t = fixture(tmp.path());
        let algo = tmp.path().join("uni/algo");
        t.insert(&[0, 0], task_at("same", &algo)).unwrap(); // task dir == container dir

        let p = plan(&t, tmp.path(), &[0, 0]).unwrap().unwrap();

        assert_eq!(p.folders, vec![algo]);
    }

    #[test]
    fn root_and_missing_paths_are_errors() {
        let tmp = tempfile::tempdir().unwrap();
        let t = fixture(tmp.path());
        assert!(plan(&t, tmp.path(), &[]).is_err());
        assert!(plan(&t, tmp.path(), &[9]).is_err());
    }

    // ---------- safety checks ----------

    /// Error text of planning a new container "big" at `dir`, added to the
    /// root of `fixture`.
    fn refusal_for(tmp: &Path, dir: &Path) -> String {
        let mut t = fixture(tmp);
        let big = container_at("big", dir, ContainerKind::Workspace, vec![]);
        let p = t.insert(&[], big).unwrap();
        plan(&t, tmp, &p).unwrap_err().to_string()
    }

    #[test]
    fn refuses_the_udo_root_and_folders_above_it() {
        let tmp = tempfile::tempdir().unwrap();
        for dir in [tmp.path().join("udo"), tmp.path().to_path_buf()] {
            let err = refusal_for(tmp.path(), &dir);
            assert!(err.starts_with("refusing: "), "{err}");
            assert!(err.contains("udo root"), "{dir:?}: {err}");
        }
    }

    #[test]
    fn refuses_home_and_folders_above_it() {
        let tmp = tempfile::tempdir().unwrap();
        let t = fixture(tmp.path());
        // homes that don't exist on disk: compared resolved all the same
        for home in [tmp.path().join("uni"), tmp.path().join("uni/me")] {
            let err = t.purge_plan_in(&[0], &home).unwrap_err().to_string();
            assert!(err.contains("contains your home folder"), "{home:?}: {err}");
        }
    }

    #[test]
    fn refuses_a_folder_holding_another_nodes_folder() {
        let tmp = tempfile::tempdir().unwrap();
        let mut t = fixture(tmp.path());
        let shared = t.get_mut(&[1]).and_then(Node::as_container_mut).unwrap();
        shared.dir = mk(tmp.path(), "uni/shared");

        let err = plan(&t, tmp.path(), &[0]).unwrap_err().to_string();

        assert!(err.contains("\"shared\" (not part of uni)"), "{err}");
    }

    #[test]
    fn refuses_a_folder_holding_an_unloaded_dir_of_another_node() {
        let tmp = tempfile::tempdir().unwrap();
        let mut t = fixture(tmp.path());
        let root = t.get_mut(&[]).and_then(Node::as_container_mut).unwrap();
        root.unloaded.push(tmp.path().join("uni/gone")); // missing on disk

        let err = plan(&t, tmp.path(), &[0]).unwrap_err().to_string();

        assert!(err.contains("unloaded container in \"root\""), "{err}");
    }

    #[test]
    fn nodes_inside_the_subtree_do_not_count_as_other() {
        let tmp = tempfile::tempdir().unwrap();
        let t = fixture(tmp.path());
        assert!(plan(&t, tmp.path(), &[0]).unwrap().is_some()); // algo, lab 3 inside uni
    }

    #[test]
    fn sibling_with_same_prefix_is_not_inside() {
        let tmp = tempfile::tempdir().unwrap();
        let mut t = fixture(tmp.path());
        let shared = t.get_mut(&[1]).and_then(Node::as_container_mut).unwrap();
        shared.dir = mk(tmp.path(), "uni2"); // "uni2" starts with "uni" as text only

        assert!(plan(&t, tmp.path(), &[0]).unwrap().is_some());
    }

    #[test]
    fn dot_dot_is_resolved() {
        let tmp = tempfile::tempdir().unwrap();
        let err = refusal_for(tmp.path(), &tmp.path().join("uni/../udo"));
        assert!(err.contains("udo root"), "{err}");
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_are_resolved() {
        use std::os::unix::fs::symlink;

        let tmp = tempfile::tempdir().unwrap();
        let t = tmp.path();
        fixture(t); // creates udo
        symlink(t.join("udo"), t.join("to-root")).unwrap();
        let err = refusal_for(t, &t.join("to-root"));
        assert!(err.contains("udo root"), "{err}");

        mk(t, "home");
        symlink(t.join("home"), t.join("to-home")).unwrap();
        let err = refusal_for(t, &t.join("to-home"));
        assert!(err.contains("home folder"), "{err}");
    }

    /// Another node stored as a link inside the folder: trashing the folder
    /// takes the link along, so that node would break. Resolved, the link
    /// points elsewhere; the stored path must count too.
    #[cfg(unix)]
    #[test]
    fn links_inside_to_other_nodes_are_refused() {
        use std::os::unix::fs::symlink;

        let tmp = tempfile::tempdir().unwrap();
        let mut t = fixture(tmp.path());
        let link = tmp.path().join("uni/shared");
        symlink(tmp.path().join("shared"), &link).unwrap();
        let shared = t.get_mut(&[1]).and_then(Node::as_container_mut).unwrap();
        shared.dir = link;

        let err = plan(&t, tmp.path(), &[0]).unwrap_err().to_string();

        assert!(err.contains("\"shared\" (not part of uni)"), "{err}");
    }

    #[cfg(unix)]
    #[test]
    fn link_inside_to_the_udo_root_is_refused() {
        use std::os::unix::fs::symlink;

        let tmp = tempfile::tempdir().unwrap();
        let mut t = fixture(tmp.path());
        let link = tmp.path().join("uni/udo-link");
        symlink(tmp.path().join("udo"), &link).unwrap();
        let root = t.get_mut(&[]).and_then(Node::as_container_mut).unwrap();
        root.dir = link;

        let err = plan(&t, tmp.path(), &[0]).unwrap_err().to_string();

        assert!(err.contains("udo root"), "{err}");
    }

    // ---------- execution ----------

    /// Fake Trash: deletes for real (inside the tempdir only).
    fn fake_trash(p: &Path) -> Result<(), String> {
        std::fs::remove_dir_all(p).map_err(|e| e.to_string())
    }

    /// Fake Trash that fails for folders called `data`.
    fn fails_on_data(p: &Path) -> Result<(), String> {
        if p.ends_with("data") {
            return Err("busy".into());
        }
        fake_trash(p)
    }

    /// `fixture` with every container's file saved (`purge` re-saves the
    /// parent, and reloading must work).
    async fn saved_fixture(tmp: &Path) -> Tree {
        let t = fixture(tmp);
        let owners: [&[usize]; 4] = [&[], &[0], &[0, 0], &[1]];
        for owner in owners {
            t.save(owner).await.unwrap();
        }
        t
    }

    async fn purge(t: &mut Tree, tmp: &Path, p: &PurgePlan, trash: TrashFn) -> Res<PurgeReport> {
        t.purge_in(p, trash, &tmp.join("home")).await
    }

    #[tokio::test]
    async fn purge_unregisters_and_trashes_exactly_the_plan() {
        let tmp = tempfile::tempdir().unwrap();
        let mut t = saved_fixture(tmp.path()).await;
        let p = plan(&t, tmp.path(), &[0]).unwrap().unwrap();

        let report = purge(&mut t, tmp.path(), &p, fake_trash).await.unwrap();

        let expected = PurgeReport {
            trashed: p.folders.clone(),
            failed: vec![],
        };
        assert_eq!(report, expected);
        assert!(!tmp.path().join("uni").exists());
        assert!(!tmp.path().join("data").exists());
        assert!(tmp.path().join("shared").exists());
        assert!(tmp.path().join("udo").join(crate::UDO_FILE_NAME).exists());
        assert_eq!(t.get(&[0]).unwrap().name(), "shared");
        let reloaded = Tree::load_from(&tmp.path().join("udo")).await.unwrap();
        assert_eq!(reloaded.resolve(&["uni"]), None);
    }

    #[tokio::test]
    async fn one_failing_folder_does_not_stop_the_rest() {
        let tmp = tempfile::tempdir().unwrap();
        let mut t = saved_fixture(tmp.path()).await;
        let p = plan(&t, tmp.path(), &[0]).unwrap().unwrap();
        let (uni, data) = (tmp.path().join("uni"), tmp.path().join("data"));

        let report = purge(&mut t, tmp.path(), &p, fails_on_data).await.unwrap();

        assert_eq!(report.trashed, vec![uni.clone()]);
        assert_eq!(report.failed, vec![(data.clone(), "busy".to_string())]);
        assert!(!uni.exists());
        assert!(data.exists());
        assert_eq!(t.resolve(&["uni"]), None); // node gone anyway
    }

    #[tokio::test]
    async fn stale_plan_changes_nothing() {
        let tmp = tempfile::tempdir().unwrap();

        // the node was replaced by another one at the same path
        let mut t = saved_fixture(tmp.path()).await;
        let p = plan(&t, tmp.path(), &[0, 0, 0]).unwrap().unwrap();
        t.get_mut(&[0, 0, 0]).unwrap().header.id = NodeId::new();
        let err = purge(&mut t, tmp.path(), &p, fake_trash).await.unwrap_err();
        assert_eq!(err.to_string(), "lab 3 changed, open the delete again");
        assert!(tmp.path().join("uni/algo/lab_3").exists());
        assert!(t.get(&[0, 0, 0]).is_some());

        // a folder appeared since the plan was made
        std::fs::remove_dir(tmp.path().join("data")).unwrap();
        let p = plan(&t, tmp.path(), &[0]).unwrap().unwrap();
        mk(tmp.path(), "data");
        assert!(purge(&mut t, tmp.path(), &p, fake_trash).await.is_err());
        assert!(tmp.path().join("uni").exists());
        assert_eq!(t.resolve(&["uni"]), Some(vec![0]));
    }
}
