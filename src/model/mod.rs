pub mod container;
pub mod data;
pub mod nav;
pub mod node;
pub mod task;
pub mod tree;
pub mod view;

use std::path::{Path, PathBuf};

use directories::BaseDirs;

/// How every node name is stored: trimmed, whitespace runs -> one space.
pub fn normalize_name(name: &str) -> String {
    name.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Folder for a node: whitespace runs -> `_`. The node name keeps its
/// spaces. None if nothing usable is left (empty / only whitespace).
pub fn folder_name(name: &str) -> Option<String> {
    let folder = name.split_whitespace().collect::<Vec<_>>().join("_");
    (!folder.is_empty()).then_some(folder)
}

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

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use crate::model::{folder_name, normalize_name, parse_abs_dir_in};

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
    fn no_whitespace_no_change() {
        let input = "test";
        let output = folder_name(input);
        assert_eq!(Some(input.to_string()), output);
    }

    #[test]
    fn collapse_multiple_whitespaces_into_underscore() {
        let input = "test    test";
        let output = folder_name(input);
        assert_eq!(Some("test_test".to_string()), output);
    }

    #[test]
    fn collapse_different_whitespaces_into_underscore() {
        let input = "test\ttest";
        let output = folder_name(input);
        assert_eq!(Some("test_test".to_string()), output);
    }

    #[test]
    fn trim_trailing_spaces() {
        let input = "test    ";
        let output = folder_name(input);
        assert_eq!(Some("test".to_string()), output);
    }

    #[test]
    fn trim_leading_spaces() {
        let input = "    test";
        let output = folder_name(input);
        assert_eq!(Some("test".to_string()), output);
    }

    #[test]
    fn trim_leading_trailing_and_combine_middle_spaces() {
        let input = "    test  test    ";
        let output = folder_name(input);
        assert_eq!(Some("test_test".to_string()), output);
    }

    #[test]
    fn empty_or_whitespace_only_has_no_folder() {
        assert_eq!(folder_name(""), None);
        assert_eq!(folder_name("  \t "), None);
    }

    #[test]
    fn normalize_name_keeps_single_spaces() {
        assert_eq!(normalize_name("  lab \t  3 "), "lab 3");
        assert_eq!(normalize_name("lab 3"), "lab 3");
        assert_eq!(normalize_name("   "), "");
    }
}
