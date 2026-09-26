pub mod container;
pub mod data;
pub mod nav;
pub mod node;
pub mod task;
pub mod tree;
pub mod view;

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

#[cfg(test)]
mod tests {
    use crate::model::{folder_name, normalize_name};

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
