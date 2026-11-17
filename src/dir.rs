use std::{
    ffi::OsString,
    path::{Path, PathBuf, absolute},
};

use directories::BaseDirs;

use crate::{DATA_ENV, ROOT_ENV, Res, naming::folder_name};

/// A dir typed in the TUI: `/…` as is, `~` / `~/…` below the home dir.
/// Anything else (relative, `~user`, empty) is an error meant for a toast.
/// Not canonicalized: the dir may not exist yet.
pub fn parse_abs_dir(input: &str) -> Result<PathBuf, String> {
    let dirs = BaseDirs::new().ok_or_else(|| "could not find the home dir".to_string())?;
    parse_abs_dir_in(input, dirs.home_dir())
}

/// `parse_abs_dir` with the home dir passed in, so tests don't depend on
/// who runs them.
fn parse_abs_dir_in(input: &str, home: &Path) -> Result<PathBuf, String> {
    let input = input.trim();
    if input.is_empty() {
        return Err("dir cannot be empty".into());
    }
    if input == "~" {
        return Ok(home.to_path_buf());
    }
    // before the general `~` check below, which would catch this too
    if let Some(rest) = input.strip_prefix("~/") {
        return Ok(home.join(rest));
    }
    if input.starts_with('~') {
        return Err("~user is not supported, use ~/… or /…".into());
    }
    let path = PathBuf::from(input);
    if !path.is_absolute() {
        return Err("dir must be absolute (/… or ~/…)".into());
    }
    Ok(path)
}

/// The two dirs udo works in: the config root (the tree, `view.toml`,
/// `run/`, `templates/`) and the data dir (`udo.db` and its copies).
#[derive(Debug, Clone, PartialEq)]
pub struct Dirs {
    pub root: PathBuf,
    pub data: PathBuf,
    /// The root is the default root, or `UDO_DATA` names the data dir: a
    /// `udo.db` still in the root gets moved to `data` (`storage::relocate`;
    /// skipped when `data` is the root itself).
    pub move_old_db: bool,
}

/// The default root and data dir of this user.
#[derive(Debug, Clone, PartialEq)]
pub struct Defaults {
    /// `~/.config/udo` on every platform (as before: not `config_dir()`).
    pub root: PathBuf,
    /// The platform data dir: `~/Library/Application Support/udo` (macOS),
    /// `$XDG_DATA_HOME/udo` or `~/.local/share/udo` (Linux).
    pub data: PathBuf,
}

/// The defaults, looked up once. None without a home dir: then only the
/// env vars give dirs.
pub fn defaults() -> Option<Defaults> {
    let base = BaseDirs::new()?;
    Some(Defaults {
        root: base.home_dir().join(".config").join("udo"),
        data: base.data_dir().join("udo"),
    })
}

/// The dirs for this process, from `$UDO_ROOT` / `$UDO_DATA`.
pub fn dirs(defaults: Option<&Defaults>) -> Res<Dirs> {
    dirs_in(
        std::env::var_os(ROOT_ENV),
        std::env::var_os(DATA_ENV),
        defaults,
    )
}

/// `dirs` with env values and defaults passed in, so tests touch neither
/// the real env nor the real home. Data dir: `UDO_DATA` if set; else the
/// default one if the root is the default root (however spelled: hooks
/// carry it explicitly); else the root itself (a custom root stays
/// self-contained). An old `udo.db` in the root moves into the data dir
/// whenever that is another dir: with the default root, or `UDO_DATA` set
/// (else its history would stay behind next to a fresh, empty one).
/// Without defaults (no home dir) `UDO_ROOT` is needed, and no root is the
/// default one.
pub fn dirs_in(
    root_env: Option<OsString>,
    data_env: Option<OsString>,
    defaults: Option<&Defaults>,
) -> Res<Dirs> {
    let root = match (set(root_env), defaults) {
        (Some(dir), _) => absolute(dir)?,
        (None, Some(defaults)) => defaults.root.clone(),
        (None, None) => return Err(format!("could not find the home dir: set {ROOT_ENV}").into()),
    };
    let default = defaults.filter(|defaults| is_same_root(&root, &defaults.root));
    let data_env = set(data_env);
    let move_old_db = default.is_some() || data_env.is_some();
    let data = match (data_env, default) {
        (Some(dir), _) => absolute(dir)?,
        (None, Some(defaults)) => defaults.data.clone(),
        (None, None) => root.clone(),
    };
    Ok(Dirs {
        root,
        data,
        move_old_db,
    })
}

/// An env value, unless unset or empty.
fn set(env: Option<OsString>) -> Option<PathBuf> {
    env.filter(|d| !d.is_empty()).map(PathBuf::from)
}

/// Is `root` the dir `real`, however spelled: the same path, or the same
/// directory on disk (a symlink, `..`, another case on a case-insensitive
/// disk). For the debug build's guard against the real data (`main.rs`).
/// A `real` that does not exist yet is only caught by its plain path.
pub fn is_same_root(root: &Path, real: &Path) -> bool {
    root == real || same_dir(root, real)
}

/// Do both paths lead to one directory (device and inode)? A path that
/// does not exist, or cannot be read, is no directory: false.
#[cfg(unix)]
fn same_dir(a: &Path, b: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    match (std::fs::metadata(a), std::fs::metadata(b)) {
        (Ok(a), Ok(b)) => (a.dev(), a.ino()) == (b.dev(), b.ino()),
        _ => false,
    }
}

/// Elsewhere no inodes: both paths resolved, if they exist.
#[cfg(not(unix))]
fn same_dir(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

/// Where a new node's folder goes by default: `<parent_dir>/<folder name>`.
/// None if nothing usable is left of the name.
pub fn default_dir(parent_dir: &Path, name: &str) -> Option<PathBuf> {
    Some(parent_dir.join(folder_name(name)?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    fn parse(input: &str) -> Result<PathBuf, String> {
        parse_abs_dir_in(input, Path::new("/home/test"))
    }

    #[test]
    fn absolute_dirs_are_kept() {
        assert_eq!(parse("/abs/dir"), Ok("/abs/dir".into()));
        assert_eq!(parse("  /abs  "), Ok("/abs".into())); // trimmed
    }

    #[test]
    fn tilde_expands_to_home() {
        assert_eq!(parse("~"), Ok("/home/test".into()));
        assert_eq!(parse("~/x/y"), Ok("/home/test/x/y".into()));
        assert_eq!(parse("~/"), Ok("/home/test".into()));
    }

    #[test]
    fn relative_tilde_user_and_empty_are_errors() {
        for input in ["rel/x", "x", "./x", "~foo", "~foo/x", "", "   "] {
            assert!(parse(input).is_err(), "{input:?}");
        }
    }

    #[test]
    fn default_dir_is_the_folder_name_below_the_parent() {
        let dir = default_dir(Path::new("/uni"), "lab  3");

        assert_eq!(dir, Some(PathBuf::from("/uni/lab_3")));
    }

    #[test]
    fn default_dir_of_a_blank_name_is_none() {
        assert_eq!(default_dir(Path::new("/uni"), "   "), None);
    }

    // ---------- is_same_root ----------

    #[test]
    fn the_real_root_is_caught_however_spelled() {
        let tmp = tempfile::tempdir().unwrap();
        let real = tmp.path().join("udo");
        std::fs::create_dir_all(real.join("sub")).unwrap();
        let link = tmp.path().join("link");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&real, &link).unwrap();

        assert!(is_same_root(&real, &real));
        assert!(is_same_root(&real.join("sub").join(".."), &real));
        #[cfg(unix)]
        assert!(is_same_root(&link, &real));
    }

    #[test]
    fn another_root_passes() {
        let tmp = tempfile::tempdir().unwrap();
        let (real, dev) = (tmp.path().join("udo"), tmp.path().join("dev"));
        std::fs::create_dir_all(real.join("sub")).unwrap();
        std::fs::create_dir_all(&dev).unwrap();

        assert!(!is_same_root(&dev, &real));
        assert!(!is_same_root(&real.join("sub"), &real)); // inside it: not the root
        assert!(!is_same_root(&tmp.path().join("missing"), &real));
    }

    /// No real root yet: only its plain path is caught (nothing to protect
    /// there, but a dev build must not create it).
    #[test]
    fn a_missing_real_root_is_caught_by_its_path() {
        let tmp = tempfile::tempdir().unwrap();
        let real = tmp.path().join("udo");

        assert!(is_same_root(&real, &real));
        assert!(!is_same_root(&tmp.path().join("other"), &real));
    }

    // ---------- dirs_in ----------

    fn defaults() -> Defaults {
        Defaults {
            root: "/home/t/.config/udo".into(),
            data: "/home/t/data/udo".into(),
        }
    }

    #[test]
    fn nothing_set_uses_both_defaults() {
        let d = defaults();
        for env in [None, Some(OsString::new())] {
            let dirs = dirs_in(env.clone(), env, Some(&d)).unwrap();
            assert_eq!(
                dirs,
                Dirs {
                    root: d.root.clone(),
                    data: d.data.clone(),
                    move_old_db: true
                }
            );
        }
    }

    #[test]
    fn custom_root_keeps_data_in_root() {
        let d = defaults();
        let dirs = dirs_in(Some("/tmp/r".into()), None, Some(&d)).unwrap();
        assert_eq!(
            dirs,
            Dirs {
                root: "/tmp/r".into(),
                data: "/tmp/r".into(),
                move_old_db: false
            }
        );
    }

    #[test]
    fn data_env_wins() {
        let d = defaults();
        for root_env in [None, Some("/tmp/r".into())] {
            let dirs = dirs_in(root_env, Some("/tmp/d".into()), Some(&d)).unwrap();
            assert_eq!(dirs.data, PathBuf::from("/tmp/d"));
        }
    }

    /// README: `$UDO_DATA` may name the default data dir; history must follow.
    /// A custom root too: else its `udo.db` stays behind and `UDO_DATA`
    /// gets a fresh, empty one.
    #[test]
    fn data_env_moves_with_any_root() {
        let d = defaults();
        let dirs = dirs_in(None, Some(d.data.clone().into()), Some(&d)).unwrap();
        assert_eq!(
            dirs,
            Dirs {
                root: d.root.clone(),
                data: d.data.clone(),
                move_old_db: true
            }
        );
        let dirs = dirs_in(None, Some("/tmp/d".into()), Some(&d)).unwrap();
        assert!(dirs.move_old_db);
        let dirs = dirs_in(Some("/tmp/r".into()), Some("/tmp/d".into()), Some(&d)).unwrap();
        assert!(dirs.move_old_db);
    }

    /// No home dir (a container, a service): the env vars alone do.
    #[test]
    fn without_a_home_dir_the_envs_suffice() {
        let dirs = dirs_in(Some("/tmp/r".into()), Some("/tmp/d".into()), None).unwrap();
        assert_eq!(
            dirs,
            Dirs {
                root: "/tmp/r".into(),
                data: "/tmp/d".into(),
                move_old_db: true
            }
        );
        let dirs = dirs_in(Some("/tmp/r".into()), None, None).unwrap();
        assert_eq!(
            dirs,
            Dirs {
                root: "/tmp/r".into(),
                data: "/tmp/r".into(),
                move_old_db: false
            }
        );
    }

    #[test]
    fn without_a_home_dir_the_root_env_is_needed() {
        let err = dirs_in(None, Some("/tmp/d".into()), None).unwrap_err();
        assert!(err.to_string().contains(ROOT_ENV), "{err}");
    }

    #[test]
    fn relative_envs_are_made_absolute() {
        let d = defaults();
        let dirs = dirs_in(Some("r".into()), Some("d".into()), Some(&d)).unwrap();
        assert!(dirs.root.is_absolute() && dirs.root.ends_with("r"));
        assert!(dirs.data.is_absolute() && dirs.data.ends_with("d"));
    }

    /// Hooks from older tmux examples carry the default root spelled their
    /// way: they must still reach the moved database.
    #[test]
    fn default_root_spelled_differently_uses_data_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("cfg").join("udo");
        std::fs::create_dir_all(&root).unwrap();
        let data = tmp.path().join("data");
        let d = Defaults {
            root: root.clone(),
            data: data.clone(),
        };
        let mut spellings = vec![OsString::from(format!("{}/", root.display()))];
        #[cfg(unix)]
        {
            let link = tmp.path().join("link");
            std::os::unix::fs::symlink(&root, &link).unwrap();
            spellings.push(link.into());
        }
        for env in spellings {
            let dirs = dirs_in(Some(env.clone()), None, Some(&d)).unwrap();
            assert_eq!(dirs.data, data, "{env:?}");
            assert!(dirs.move_old_db, "{env:?}");
        }
    }
}
