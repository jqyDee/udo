use crate::{
    dir::parse_abs_dir,
    model::{
        settings::{ContainerSettings, Resolved, RootSettings, Source},
        tree::Tree,
    },
};

/// One setting as the UI sees it: its name and its value as text. `S`:
/// `ContainerSettings` (`SETTINGS`) or `RootSettings` (`ROOT_SETTINGS`).
pub struct SettingInfo<S> {
    /// Key in `.udo.toml` (root settings: in the `[root]` table).
    pub key: &'static str,
    /// Shown in the UI.
    pub label: &'static str,
    /// The value set in this file as text (None = not set here).
    pub get: fn(&S) -> Option<String>,
    /// Parse text into the value set in this file; blank = unset
    /// (container settings: inherit). Err: message for a toast, the
    /// settings stay unchanged.
    pub set: fn(&mut S, &str) -> Result<(), String>,
}

/// Every container setting, in the order the UI shows them.
pub const SETTINGS: &[SettingInfo<ContainerSettings>] = &[
    SettingInfo {
        key: "task_folders",
        label: "task folders",
        get: |s| s.task_folders.map(|v| v.to_string()),
        set: |s, text| {
            s.task_folders = opt(text, str::parse)?;
            Ok(())
        },
    },
    SettingInfo {
        key: "default_deadline",
        label: "deadline",
        get: |s| s.default_deadline.map(|v| v.to_string()),
        set: |s, text| {
            s.default_deadline = opt(text, str::parse)?;
            Ok(())
        },
    },
    SettingInfo {
        key: "archive_dir",
        label: "archive",
        get: |s| s.archive_dir.as_ref().map(|p| p.display().to_string()),
        set: |s, text| {
            s.archive_dir = opt(text, parse_abs_dir)?;
            Ok(())
        },
    },
];

/// Every root-only setting, in the order the UI shows them. Not inherited.
pub const ROOT_SETTINGS: &[SettingInfo<RootSettings>] = &[SettingInfo {
    key: "theme",
    label: "theme",
    get: |s| s.theme.clone(),
    set: |s, text| {
        s.theme = opt(text, |t| Ok(t.to_string()))?;
        Ok(())
    },
}];

/// Blank -> None (unset), else `parse` on the trimmed text.
fn opt<T>(text: &str, parse: impl Fn(&str) -> Result<T, String>) -> Result<Option<T>, String> {
    let text = text.trim();
    if text.is_empty() {
        Ok(None)
    } else {
        parse(text).map(Some)
    }
}

/// One row of the settings view: label, effective value, where it came from.
pub struct EffectiveSetting {
    pub label: &'static str,
    /// None: set nowhere and no default.
    pub value: Option<Resolved<String>>,
}

impl Tree {
    /// Every setting in `SETTINGS` for the node at `path` (task: its
    /// container), resolved like `setting`.
    pub fn effective_settings(&self, path: &[usize]) -> Vec<EffectiveSetting> {
        SETTINGS
            .iter()
            .map(|info| EffectiveSetting {
                label: info.label,
                value: self.setting(path, info.get),
            })
            .collect()
    }

    /// Where a value came from, for the UI: `own`, `from uni`, `default`.
    pub fn source_text(&self, source: &Source) -> String {
        match source {
            Source::Own => "own".into(),
            Source::Inherited(path) => {
                format!("from {}", self.get(path).map_or("?", |n| n.name()))
            }
            Source::Default => "default".into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use serde::Serialize;

    use super::*;
    use crate::{
        model::{
            node::Node,
            settings::{Source, TaskFolderSetting},
        },
        test_util::{container, task, tree_with},
    };

    /// Every field set, so each one shows up in the file.
    fn all_set() -> ContainerSettings {
        ContainerSettings {
            archive_dir: Some(PathBuf::from("/arch")),
            task_folders: Some(TaskFolderSetting::Auto),
            default_deadline: Some("fri 22:00".parse().unwrap()),
        }
    }

    fn all_root_set() -> RootSettings {
        RootSettings {
            theme: Some("dark".into()),
        }
    }

    // ---------- checks for both tables (`full`: every field set) ----------

    fn check_keys_are_the_fields<S: Serialize>(table: &[SettingInfo<S>], full: &S) {
        let text = toml::to_string(full).unwrap();
        for info in table {
            assert!(
                text.contains(&format!("{} = ", info.key)),
                "{} not in:\n{text}",
                info.key
            );
        }
        // and nothing in the file is missing from the table
        assert_eq!(text.lines().count(), table.len(), "got:\n{text}");
    }

    fn check_set_reads_back_get<S: Serialize + Default>(table: &[SettingInfo<S>], full: &S) {
        let mut s = S::default();
        for info in table {
            let text = (info.get)(full).unwrap();
            (info.set)(&mut s, &text).unwrap();
        }
        let file = |s: &S| toml::to_string(s).unwrap();
        assert_eq!(file(&s), file(full));
    }

    fn check_blank_unsets<S>(table: &[SettingInfo<S>], mut full: S) {
        for info in table {
            (info.set)(&mut full, "  ").unwrap();
            assert_eq!((info.get)(&full), None, "{}", info.key);
        }
    }

    #[test]
    fn every_setting_key_is_a_field() {
        check_keys_are_the_fields(SETTINGS, &all_set());
        check_keys_are_the_fields(ROOT_SETTINGS, &all_root_set());
    }

    #[test]
    fn get_shows_the_value_as_it_is_written() {
        let s = all_set();
        let texts: Vec<_> = SETTINGS.iter().map(|i| (i.get)(&s)).collect();
        assert_eq!(
            texts,
            [
                Some("auto".to_string()),
                Some("fri 22:00".to_string()),
                Some("/arch".to_string()),
            ]
        );
        let empty = ContainerSettings::default();
        assert!(SETTINGS.iter().all(|i| (i.get)(&empty).is_none()));
    }

    #[test]
    fn root_get_shows_the_value_as_it_is_written() {
        let texts: Vec<_> = ROOT_SETTINGS
            .iter()
            .map(|i| (i.get)(&all_root_set()))
            .collect();
        assert_eq!(texts, [Some("dark".to_string())]);
        let empty = RootSettings::default();
        assert!(ROOT_SETTINGS.iter().all(|i| (i.get)(&empty).is_none()));
    }

    #[test]
    fn set_reads_back_what_get_shows() {
        check_set_reads_back_get(SETTINGS, &all_set());
        check_set_reads_back_get(ROOT_SETTINGS, &all_root_set());
    }

    #[test]
    fn set_blank_unsets() {
        check_blank_unsets(SETTINGS, all_set());
        check_blank_unsets(ROOT_SETTINGS, all_root_set());
    }

    #[test]
    fn set_trims() {
        let mut s = ContainerSettings::default();
        let info = SETTINGS.iter().find(|i| i.key == "task_folders").unwrap();
        (info.set)(&mut s, " auto ").unwrap();
        assert_eq!(s.task_folders, Some(TaskFolderSetting::Auto));
    }

    #[test]
    fn set_rejects_bad_input_and_keeps_the_value() {
        let bad = [
            ("task_folders", "custom"),
            ("default_deadline", "someday"),
            ("archive_dir", "rel/path"),
        ];
        for (key, text) in bad {
            let info = SETTINGS.iter().find(|i| i.key == key).unwrap();
            let mut s = all_set();
            assert!((info.set)(&mut s, text).is_err(), "{key} = {text:?}");
            assert_eq!((info.get)(&s), (info.get)(&all_set()), "{key} changed");
        }
    }

    #[test]
    fn effective_settings_lists_all_in_order_with_sources() {
        // root: [uni: [cs: [lab]]]
        let mut t = tree_with(vec![container(
            "uni",
            vec![container("cs", vec![task("lab")])],
        )]);
        let uni = t.get_mut(&[0]).and_then(Node::as_container_mut).unwrap();
        uni.settings.default_deadline = Some("fri 22:00".parse().unwrap());

        let rows = t.effective_settings(&[0, 0, 0]); // task "lab"

        let labels: Vec<_> = rows.iter().map(|r| r.label).collect();
        let expected: Vec<_> = SETTINGS.iter().map(|i| i.label).collect();
        assert_eq!(labels, expected);

        let by_label = |label: &str| rows.iter().find(|r| r.label == label).unwrap();
        assert_eq!(
            by_label("deadline").value,
            Some(Resolved {
                value: "fri 22:00".into(),
                source: Source::Inherited(vec![0]),
            })
        );
        assert_eq!(
            by_label("task folders").value.as_ref().unwrap().source,
            Source::Default
        );
        assert_eq!(by_label("archive").value, None); // no default
    }
}
