use std::{
    ffi::OsString,
    path::{Path, PathBuf, absolute},
};

use directories::BaseDirs;

use crate::{ROOT_ENV, Res};

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

/// `$UDO_ROOT` if set and non-empty, else `~/.config/udo`.
pub fn root_dir() -> Res<PathBuf> {
    root_dir_from(std::env::var_os(ROOT_ENV))
}

/// `root_dir` with the env value passed in, so tests don't touch the real env.
pub fn root_dir_from(env: Option<OsString>) -> Res<PathBuf> {
    if let Some(dir) = env.filter(|d| !d.is_empty()) {
        // absolute: children paths are stored absolute, cwd must not matter
        return Ok(absolute(PathBuf::from(dir))?);
    }
    let base_dirs = BaseDirs::new().ok_or("Could not acquire Base dirs")?;
    Ok(base_dirs.home_dir().join(".config").join("udo"))
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
}
