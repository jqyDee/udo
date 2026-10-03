//! `NODE` arguments -> tree paths. One rule for every command: nothing =
//! the node whose folder you are in; `id:<uuid>` = the node with that ID
//! (for scripts: survives renames); `/` = the root; a leading `/` = the
//! exact path from the root; else a path of names or any unique ending of
//! one (`lab 3`, `cs/lab 3`).

use std::path::{Path, PathBuf};

use crate::{
    Res,
    model::{NodePath, id::NodeId, node::Node, tree::Tree},
    naming::normalize_name,
};

/// The node `arg` names; `None` = the node of `cwd`'s folder. An ID no
/// node has (deleted) is "not found", not read as names.
pub fn resolve(tree: &Tree, arg: Option<&str>, cwd: &Path) -> Res<NodePath> {
    let Some(arg) = arg else {
        return cwd_node(tree, cwd).ok_or_else(|| "not in a udo folder, name a node".into());
    };
    let trimmed = arg.trim();
    if let Some(id) = node_id(trimmed) {
        return tree
            .path_of(id)
            .ok_or_else(|| format!("not found: {arg:?}").into());
    }
    if trimmed == "/" {
        return Ok(vec![]);
    }
    if let Some(from_root) = trimmed.strip_prefix('/') {
        let names = parts(from_root)?;
        return exact(tree, &names).ok_or_else(|| format!("not found: {arg:?}").into());
    }
    let names = parts(trimmed)?;
    if let Some(path) = exact(tree, &names) {
        return Ok(path); // an exact path from the root always wins
    }
    let found: Vec<NodePath> = tree
        .rows()
        .into_iter()
        .map(|r| r.path)
        .filter(|p| names_of(tree, p).ends_with(&names))
        .collect();
    match found.as_slice() {
        [] => Err(format!("not found: {arg:?}").into()),
        [one] => Ok(one.clone()),
        many => {
            // several: the one below where you are, if exactly one is
            if let Some(here) = cwd_node(tree, cwd) {
                let inside: Vec<&NodePath> = many.iter().filter(|p| p.starts_with(&here)).collect();
                if let [one] = inside.as_slice() {
                    return Ok((*one).clone());
                }
            }
            let list: Vec<String> = many
                .iter()
                .map(|p| format!("  {}", path_text(tree, p)))
                .collect();
            Err(format!("{arg:?} matches {} nodes:\n{}", many.len(), list.join("\n")).into())
        }
    }
}

/// Where a new node `arg` goes, and its name: the last part is the new
/// name, the parts before it name the parent (like `resolve`). Only a
/// name: the container of `cwd`'s node (a task: its container), or the root
/// outside udo folders.
pub fn resolve_parent(tree: &Tree, arg: &str, cwd: &Path) -> Res<(NodePath, String)> {
    let trimmed = arg.trim();
    let (parent_arg, name) = match trimmed.rsplit_once('/') {
        Some((parent, name)) => (Some(parent), name),
        None => (None, trimmed),
    };
    let name = normalize_name(name);
    if name.is_empty() {
        return Err("name cannot be empty".into());
    }
    let parent = match parent_arg {
        Some("") => vec![], // "/lab 4": in the root
        Some(parent) => resolve(tree, Some(parent), cwd)?,
        None => cwd_node(tree, cwd)
            .and_then(|p| tree.nearest_file_owner(&p))
            .unwrap_or_default(),
    };
    if tree.get(&parent).and_then(Node::as_container).is_none() {
        return Err(format!("not a container: {}", path_text(tree, &parent)).into());
    }
    Ok((parent, name))
}

/// `uni/cs/lab 3`: the names from below the root; the root itself is `/`.
pub fn path_text(tree: &Tree, path: &[usize]) -> String {
    if path.is_empty() {
        return "/".into();
    }
    names_of(tree, path).join("/")
}

/// The node whose folder contains `cwd`; the deepest folder wins.
pub fn cwd_node(tree: &Tree, cwd: &Path) -> Option<NodePath> {
    let cwd = real(cwd);
    std::iter::once(vec![])
        .chain(tree.rows().into_iter().map(|r| r.path))
        .filter_map(|path| {
            let dir = real(tree.get(&path)?.dir()?);
            cwd.starts_with(&dir)
                .then(|| (dir.components().count(), path))
        })
        .max_by_key(|(depth, _)| *depth)
        .map(|(_, path)| path)
}

/// `path` with symlinks resolved if it exists (on macOS `/tmp` is
/// `/private/tmp`, and `current_dir` gives the resolved form), else as is.
fn real(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

/// The names of `arg`, split at `/` and cleaned like stored names.
fn parts(arg: &str) -> Res<Vec<String>> {
    arg.split('/')
        .map(|part| match normalize_name(part) {
            name if name.is_empty() => Err(format!("empty name in {arg:?}").into()),
            name => Ok(name),
        })
        .collect()
}

/// The node at exactly `names` from the root.
fn exact(tree: &Tree, names: &[String]) -> Option<NodePath> {
    let names: Vec<&str> = names.iter().map(String::as_str).collect();
    tree.resolve(&names)
}

/// The names from below the root down to the node at `path`.
fn names_of(tree: &Tree, path: &[usize]) -> Vec<String> {
    (1..=path.len())
        .filter_map(|i| tree.get(&path[..i]).map(|n| n.name().to_string()))
        .collect()
}

/// `id:<uuid>` -> the ID. Anything else (`id:` + not an ID, a plain name):
/// `None`, read as names as before. A node named `id:<a valid uuid>` is
/// then only reachable by its path.
fn node_id(arg: &str) -> Option<NodeId> {
    arg.strip_prefix("id:")?.trim().parse().ok()
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;
    use crate::{
        model::{container::ContainerKind, task::Task, time},
        test_util::container_at,
    };

    /// root (tmp) -> [
    ///   uni (tmp/uni) -> [
    ///     cs (tmp/uni/cs) -> [lab 3 (tmp/uni/cs/lab_3)],
    ///     physics (tmp/uni/physics) -> [lab 3 (no folder)],
    ///   ],
    ///   notes (task, no folder),
    /// ]
    fn tree() -> (TempDir, Tree) {
        let tmp = tempfile::tempdir().unwrap();
        let d = |p: &str| -> PathBuf {
            let dir = tmp.path().join(p);
            std::fs::create_dir_all(&dir).unwrap();
            dir
        };
        let task =
            |name: &str, dir: Option<PathBuf>| Node::task(name.into(), Task::new(dir, time::now()));
        let cs = container_at(
            "cs",
            &d("uni/cs"),
            ContainerKind::Project,
            vec![task("lab 3", Some(d("uni/cs/lab_3")))],
        );
        let physics = container_at(
            "physics",
            &d("uni/physics"),
            ContainerKind::Project,
            vec![task("lab 3", None)],
        );
        let uni = container_at("uni", &d("uni"), ContainerKind::Workspace, vec![cs, physics]);
        let root =
            container_at("root", tmp.path(), ContainerKind::Root, vec![uni, task("notes", None)]);
        (tmp, Tree::new(root))
    }

    /// A folder outside the tree.
    fn elsewhere() -> TempDir {
        tempfile::tempdir().unwrap()
    }

    #[test]
    fn no_argument_takes_the_deepest_folder_around_cwd() {
        let (tmp, t) = tree();
        let src = tmp.path().join("uni/cs/lab_3/src");
        std::fs::create_dir_all(&src).unwrap();

        assert_eq!(resolve(&t, None, &src).unwrap(), vec![0, 0, 0]);
        assert_eq!(resolve(&t, None, &tmp.path().join("uni")).unwrap(), vec![0]);
    }

    #[test]
    fn no_argument_resolves_symlinked_folders() {
        let (tmp, t) = tree();
        // what `current_dir` gives on macOS: the resolved form
        let cwd = tmp.path().join("uni/cs").canonicalize().unwrap();

        assert_eq!(resolve(&t, None, &cwd).unwrap(), vec![0, 0]);
    }

    #[test]
    fn no_argument_outside_udo_is_an_error() {
        let (_tmp, t) = tree();
        let out = elsewhere();

        let err = resolve(&t, None, out.path()).unwrap_err().to_string();

        assert!(err.contains("not in a udo folder"), "{err}");
    }

    #[test]
    fn slash_is_the_root() {
        let (_tmp, t) = tree();

        assert_eq!(resolve(&t, Some("/"), elsewhere().path()).unwrap(), Vec::<usize>::new());
    }

    #[test]
    fn a_full_path_and_a_unique_suffix() {
        let (_tmp, t) = tree();
        let out = elsewhere();

        assert_eq!(resolve(&t, Some("uni/cs/lab 3"), out.path()).unwrap(), vec![0, 0, 0]);
        assert_eq!(resolve(&t, Some("cs/lab 3"), out.path()).unwrap(), vec![0, 0, 0]);
        assert_eq!(resolve(&t, Some("physics"), out.path()).unwrap(), vec![0, 1]);
    }

    #[test]
    fn whitespace_in_the_argument_is_cleaned_like_names() {
        let (_tmp, t) = tree();

        assert_eq!(resolve(&t, Some(" uni /  cs "), elsewhere().path()).unwrap(), vec![0, 0]);
    }

    #[test]
    fn a_leading_slash_means_the_exact_path_only() {
        let (_tmp, t) = tree();
        let out = elsewhere();

        assert_eq!(resolve(&t, Some("/uni/cs"), out.path()).unwrap(), vec![0, 0]);
        assert!(resolve(&t, Some("/cs"), out.path()).is_err()); // a suffix, not from the root
    }

    #[test]
    fn an_exact_path_beats_a_suffix_elsewhere() {
        let (_tmp, mut t) = tree();
        let inner_uni =
            container_at("uni", Path::new("/tmp/other/uni"), ContainerKind::Project, vec![]);
        let other = container_at(
            "other",
            Path::new("/tmp/other"),
            ContainerKind::Workspace,
            vec![inner_uni],
        );
        t.root.as_container_mut().unwrap().children.push(other);

        assert_eq!(resolve(&t, Some("uni"), elsewhere().path()).unwrap(), vec![0]);
    }

    #[test]
    fn an_ambiguous_name_lists_the_candidates() {
        let (_tmp, t) = tree();

        let err = resolve(&t, Some("lab 3"), elsewhere().path())
            .unwrap_err()
            .to_string();

        assert_eq!(err, "\"lab 3\" matches 2 nodes:\n  uni/cs/lab 3\n  uni/physics/lab 3");
    }

    #[test]
    fn an_ambiguous_name_takes_the_one_below_cwd() {
        let (tmp, t) = tree();

        let found = resolve(&t, Some("lab 3"), &tmp.path().join("uni/physics")).unwrap();

        assert_eq!(found, vec![0, 1, 0]);
    }

    #[test]
    fn an_unknown_name_is_not_found() {
        let (_tmp, t) = tree();

        let err = resolve(&t, Some("lab 9"), elsewhere().path())
            .unwrap_err()
            .to_string();

        assert_eq!(err, "not found: \"lab 9\"");
    }

    /// `physics/lab 3` by its ID: also settles the ambiguous `lab 3`.
    #[test]
    fn an_id_finds_its_node() {
        let (_tmp, t) = tree();
        let out = elsewhere();
        let id = |path: &[usize]| format!("id:{}", t.get(path).unwrap().id());

        assert_eq!(resolve(&t, Some(&id(&[0, 1, 0])), out.path()).unwrap(), vec![0, 1, 0]);
        assert_eq!(resolve(&t, Some(&id(&[])), out.path()).unwrap(), Vec::<usize>::new());
        let spaced = format!("  {}  ", id(&[0, 0]));
        assert_eq!(resolve(&t, Some(&spaced), out.path()).unwrap(), vec![0, 0]);
    }

    #[test]
    fn an_unknown_id_is_not_found() {
        let (_tmp, t) = tree();
        let ghost = format!("id:{}", NodeId::new());

        let err = resolve(&t, Some(&ghost), elsewhere().path())
            .unwrap_err()
            .to_string();

        assert_eq!(err, format!("not found: {ghost:?}"));
    }

    /// `id:` + something that is no ID: a name like any other.
    #[test]
    fn id_without_an_id_is_read_as_names() {
        let (_tmp, mut t) = tree();
        let odd = Node::task("id:x".into(), Task::new(None, time::now()));
        t.root.as_container_mut().unwrap().children.push(odd);

        assert_eq!(resolve(&t, Some("id:x"), elsewhere().path()).unwrap(), vec![2]);
        assert!(resolve(&t, Some("id:"), elsewhere().path()).is_err()); // no such name either
    }

    /// The parent part goes through `resolve`, so it can be an ID too.
    #[test]
    fn parent_by_id() {
        let (_tmp, t) = tree();
        let cs = format!("id:{}", t.get(&[0, 0]).unwrap().id());

        let (parent, name) = resolve_parent(&t, &format!("{cs}/lab 4"), elsewhere().path()).unwrap();

        assert_eq!((parent, name), (vec![0, 0], "lab 4".to_string()));
    }

    #[test]
    fn parent_of_a_new_path() {
        let (_tmp, t) = tree();
        let out = elsewhere();

        assert_eq!(
            resolve_parent(&t, "uni/cs/lab 4", out.path()).unwrap(),
            (vec![0, 0], "lab 4".into())
        );
        assert_eq!(resolve_parent(&t, "/lab 4", out.path()).unwrap(), (vec![], "lab 4".into()));
        assert_eq!(resolve_parent(&t, "lab 4", out.path()).unwrap(), (vec![], "lab 4".into()));
    }

    /// Standing in a task's folder: the new node goes into its container.
    #[test]
    fn parent_from_cwd_in_a_task_folder_is_its_container() {
        let (tmp, t) = tree();

        let (parent, _) = resolve_parent(&t, "lab 4", &tmp.path().join("uni/cs/lab_3")).unwrap();

        assert_eq!(parent, vec![0, 0]);
    }

    #[test]
    fn a_task_cannot_be_a_parent() {
        let (_tmp, t) = tree();

        let err = resolve_parent(&t, "notes/x", elsewhere().path())
            .unwrap_err()
            .to_string();

        assert!(err.contains("not a container"), "{err}");
    }

    #[test]
    fn path_text_names_the_path() {
        let (_tmp, t) = tree();

        assert_eq!(path_text(&t, &[0, 1, 0]), "uni/physics/lab 3");
        assert_eq!(path_text(&t, &[]), "/");
    }
}
