use std::{
    collections::BTreeMap,
    fs, io,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};

use crate::{model::settings::RunName, run::RunError};

/// The run configs in one folder: every executable file directly in it,
/// by name (its file stem: `idea.py` -> `idea`). Subfolders are not searched
/// (room for `lib/` helpers); hidden files and stems that are no `RunName`
/// are skipped. Symlinks count as what they point to (dotfiles).
#[derive(Debug)]
pub struct Library {
    dir: PathBuf,
    /// Executable files by name. More than one: a duplicate, reported by
    /// `find` (loading still works, the other scripts stay usable).
    scripts: BTreeMap<RunName, Vec<PathBuf>>,
    /// Files that would be scripts but lack `+x`: `find` says how to fix
    /// it instead of "not found".
    not_executable: BTreeMap<RunName, PathBuf>,
}

impl Library {
    /// Read `dir`. A missing folder is an empty library, not an error (no
    /// scripts yet); one that exists but cannot be read is `Io`.
    pub fn load(dir: &Path) -> Result<Library, RunError> {
        let mut library = Library {
            dir: dir.to_path_buf(),
            scripts: BTreeMap::new(),
            not_executable: BTreeMap::new(),
        };
        let io_error = |error| RunError::Io {
            dir: dir.to_path_buf(),
            error,
        };
        let entries = match fs::read_dir(dir) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(library),
            other => other.map_err(io_error)?,
        };
        for entry in entries {
            let path = entry.map_err(io_error)?.path();
            let Some(name) = script_name(&path) else {
                continue;
            };
            // `fs::metadata` follows symlinks; a broken one is skipped
            let Ok(meta) = fs::metadata(&path) else {
                continue;
            };
            if !meta.is_file() {
                continue; // subfolders: helpers, not scripts
            }
            if meta.permissions().mode() & 0o111 != 0 {
                library.scripts.entry(name).or_default().push(path);
            } else {
                library.not_executable.insert(name, path);
            }
        }
        for paths in library.scripts.values_mut() {
            paths.sort(); // `read_dir` has no fixed order
        }
        Ok(library)
    }

    /// The folder it was read from.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// The names of the scripts that can run, sorted: for the picker and
    /// `udo run --list`. Duplicates are listed (once); `find` reports them.
    pub fn names(&self) -> impl Iterator<Item = &RunName> {
        self.scripts.keys()
    }

    /// The script called `name`.
    pub fn find(&self, name: &RunName) -> Result<&Path, RunError> {
        match self.scripts.get(name).map(Vec::as_slice) {
            Some([one]) => Ok(one),
            Some(many) => Err(RunError::Duplicate {
                name: name.clone(),
                paths: many.to_vec(),
            }),
            None => match self.not_executable.get(name) {
                Some(path) => Err(RunError::NotExecutable { path: path.clone() }),
                None => Err(RunError::NotFound {
                    name: name.clone(),
                    dir: self.dir.clone(),
                    have: self.names().cloned().collect(),
                }),
            },
        }
    }
}

/// The name `path` would have as a script: its stem, if that is a
/// `RunName`. Hidden files (`.DS_Store`, `.keep`): none.
fn script_name(path: &Path) -> Option<RunName> {
    let file = path.file_name()?.to_str()?;
    if file.starts_with('.') {
        return None;
    }
    path.file_stem()?.to_str()?.parse().ok()
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::symlink;

    use tempfile::TempDir;

    use super::*;

    /// A script `file` in `dir`, executable or not.
    fn script(dir: &Path, file: &str, executable: bool) -> PathBuf {
        let path = dir.join(file);
        fs::write(&path, "#!/bin/sh\n").unwrap();
        let mode = if executable { 0o755 } else { 0o644 };
        fs::set_permissions(&path, fs::Permissions::from_mode(mode)).unwrap();
        path
    }

    fn name(s: &str) -> RunName {
        s.parse().unwrap()
    }

    fn names(library: &Library) -> Vec<&str> {
        library.names().map(RunName::as_str).collect()
    }

    fn folder() -> TempDir {
        tempfile::tempdir().unwrap()
    }

    #[test]
    fn executable_files_by_stem_sorted() {
        let dir = folder();
        script(dir.path(), "nvim-tmux", true);
        let idea = script(dir.path(), "idea.py", true);

        let library = Library::load(dir.path()).unwrap();

        assert_eq!(names(&library), ["idea", "nvim-tmux"]);
        assert_eq!(library.find(&name("idea")).unwrap(), idea); // extension kept
        assert_eq!(library.dir(), dir.path());
    }

    #[test]
    fn a_missing_folder_is_an_empty_library() {
        let dir = folder();
        let missing = dir.path().join("run");

        let library = Library::load(&missing).unwrap();

        assert!(names(&library).is_empty());
        let err = library.find(&name("idea")).unwrap_err().to_string();
        assert_eq!(err, format!("no run config \"idea\" in {} (none yet)", missing.display()));
    }

    #[test]
    fn an_unknown_name_lists_the_names_there_are() {
        let dir = folder();
        script(dir.path(), "idea", true);
        script(dir.path(), "nvim-tmux", true);

        let err = Library::load(dir.path())
            .unwrap()
            .find(&name("idae"))
            .unwrap_err();

        assert!(matches!(err, RunError::NotFound { .. }));
        assert!(err.to_string().ends_with("(have: idea, nvim-tmux)"), "{err}");
    }

    /// Two files, one name: `find` refuses it, the others still work.
    #[test]
    fn a_duplicate_names_both_files() {
        let dir = folder();
        script(dir.path(), "idea.sh", true);
        script(dir.path(), "idea.py", true);
        script(dir.path(), "zed", true);

        let library = Library::load(dir.path()).unwrap();

        let err = library.find(&name("idea")).unwrap_err();
        assert_eq!(
            err.to_string(),
            "two run configs named \"idea\": idea.py, idea.sh (rename one)"
        );
        assert!(library.find(&name("zed")).is_ok());
        assert_eq!(names(&library), ["idea", "zed"]);
    }

    #[test]
    fn without_x_it_is_not_offered_and_says_how_to_fix_it() {
        let dir = folder();
        let path = script(dir.path(), "idea.py", false);

        let library = Library::load(dir.path()).unwrap();

        assert!(names(&library).is_empty());
        let err = library.find(&name("idea")).unwrap_err();
        assert_eq!(
            err.to_string(),
            format!("run config \"idea\" is not executable: chmod +x {}", path.display())
        );
    }

    #[test]
    fn subfolders_hidden_files_and_odd_names_are_skipped() {
        let dir = folder();
        fs::create_dir(dir.path().join("lib")).unwrap();
        script(&dir.path().join("lib"), "helper", true);
        script(dir.path(), ".hidden", true);
        script(dir.path(), "my script.sh", true);
        script(dir.path(), "none", true); // reserved: switches settings off
        script(dir.path(), "idea", true);

        let library = Library::load(dir.path()).unwrap();

        assert_eq!(names(&library), ["idea"]);
    }

    /// Scripts kept in a dotfiles repo, linked into `run_dir`.
    #[test]
    fn a_symlink_counts_as_what_it_points_to() {
        let (dir, dotfiles) = (folder(), folder());
        let real = script(dotfiles.path(), "idea.sh", true);
        symlink(&real, dir.path().join("idea.sh")).unwrap();
        symlink(dotfiles.path().join("gone"), dir.path().join("broken")).unwrap();

        let library = Library::load(dir.path()).unwrap();

        assert_eq!(names(&library), ["idea"]);
        assert_eq!(library.find(&name("idea")).unwrap(), dir.path().join("idea.sh"));
    }

    #[test]
    fn an_unreadable_folder_is_an_error() {
        let dir = folder();
        let file = script(dir.path(), "not-a-folder", true);

        let err = Library::load(&file).unwrap_err();

        assert!(matches!(err, RunError::Io { .. }), "{err}");
        assert!(err.to_string().starts_with("cannot read "), "{err}");
    }
}
