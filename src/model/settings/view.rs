use crate::{
    dir::parse_abs_dir,
    model::{
        settings::{ContainerSettings, Resolved, RootSettings, Source, TaskFolderSetting},
        time::{DeadlineRule, Minutes},
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
    /// Fixed values (as `get` shows them): the form offers a choice instead
    /// of free text. Empty: free text.
    pub choices: &'static [&'static str],
    /// Examples of how values are written, `·`-separated (each one is
    /// accepted by `set`, tested). Shown below the form while the field is
    /// active. None: obvious (e.g. a choice).
    pub format: Option<&'static str>,
}

/// Every container setting, in the order the UI shows them.
pub const SETTINGS: &[SettingInfo<ContainerSettings>] = &[
    SettingInfo {
        key: "task_folders",
        label: "task folders",
        choices: TaskFolderSetting::LABELS,
        format: None,
        get: |s| s.task_folders.map(|v| v.to_string()),
        set: |s, text| {
            s.task_folders = opt(text, str::parse)?;
            Ok(())
        },
    },
    SettingInfo {
        key: "default_deadline",
        label: "deadline",
        choices: &[],
        format: Some(DeadlineRule::EXAMPLES),
        get: |s| s.default_deadline.map(|v| v.to_string()),
        set: |s, text| {
            s.default_deadline = opt(text, str::parse)?;
            Ok(())
        },
    },
    SettingInfo {
        key: "archive_dir",
        label: "archive",
        choices: &[],
        format: Some("/path/to/dir · ~/dir"),
        get: |s| s.archive_dir.as_ref().map(|p| p.display().to_string()),
        set: |s, text| {
            s.archive_dir = opt(text, parse_abs_dir)?;
            Ok(())
        },
    },
    SettingInfo {
        key: "estimate",
        label: "estimate",
        choices: &[],
        format: Some(Minutes::EXAMPLES),
        get: |s| s.estimate.map(|e| e.to_string()),
        set: |s, text| {
            s.estimate = opt(text, str::parse)?;
            Ok(())
        },
    },
    // free text, not `choices`: the names are the scripts in `run_dir`,
    // known only at runtime; an unknown one fails when run, not here
    SettingInfo {
        key: "open_with",
        label: "open with",
        choices: &[],
        format: Some("nvim-tmux · none"),
        get: |s| s.open_with.as_ref().map(|v| v.to_string()),
        set: |s, text| {
            s.open_with = opt(text, str::parse)?;
            Ok(())
        },
    },
    SettingInfo {
        key: "on_create",
        label: "on create",
        choices: &[],
        format: Some("typst-setup · none"),
        get: |s| s.on_create.as_ref().map(|v| v.to_string()),
        set: |s, text| {
            s.on_create = opt(text, str::parse)?;
            Ok(())
        },
    },
];

/// Every root-only setting, in the order the UI shows them. Not inherited.
pub const ROOT_SETTINGS: &[SettingInfo<RootSettings>] = &[
    SettingInfo {
        key: "theme",
        label: "theme",
        choices: &[],
        format: None,
        get: |s| s.theme.clone(),
        set: |s, text| {
            s.theme = opt(text, |t| Ok(t.to_string()))?;
            Ok(())
        },
    },
    SettingInfo {
        key: "run_dir",
        label: "run configs",
        choices: &[],
        format: Some("/path/to/dir · ~/dir"),
        get: |s| s.run_dir.as_ref().map(|p| p.display().to_string()),
        set: |s, text| {
            s.run_dir = opt(text, parse_abs_dir)?;
            Ok(())
        },
    },
];

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
    /// container), resolved like `setting`. If that container is the root,
    /// then every `ROOT_SETTINGS` entry too (own or unset, never inherited):
    /// the same fields its settings form shows.
    pub fn effective_settings(&self, path: &[usize]) -> Vec<EffectiveSetting> {
        let mut rows: Vec<_> = SETTINGS
            .iter()
            .map(|info| EffectiveSetting {
                label: info.label,
                value: self.setting(path, info.get),
            })
            .collect();
        if self.nearest_file_owner(path).is_some_and(|p| p.is_empty()) {
            rows.extend(ROOT_SETTINGS.iter().map(|info| EffectiveSetting {
                label: info.label,
                value: (info.get)(self.root_settings()).map(|value| Resolved {
                    value,
                    source: Source::Own,
                }),
            }));
        }
        rows
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
            time::Minutes,
        },
        test_util::{container, task, tree_with},
    };

    /// Every field set, so each one shows up in the file.
    fn all_set() -> ContainerSettings {
        ContainerSettings {
            archive_dir: Some(PathBuf::from("/arch")),
            task_folders: Some(TaskFolderSetting::Auto),
            default_deadline: Some("fri 22:00".parse().unwrap()),
            estimate: Some(Minutes::new(10)),
            open_with: Some("nvim-tmux".parse().unwrap()),
            on_create: Some("none".parse().unwrap()),
        }
    }

    fn all_root_set() -> RootSettings {
        RootSettings {
            theme: Some("dark".into()),
            run_dir: Some(PathBuf::from("/dotfiles/run")),
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

    fn check_choices_read_back<S: Default>(table: &[SettingInfo<S>]) {
        for info in table {
            for &choice in info.choices {
                let mut s = S::default();
                (info.set)(&mut s, choice).unwrap();
                assert_eq!((info.get)(&s).as_deref(), Some(choice), "{}", info.key);
            }
        }
    }

    #[test]
    fn every_choice_is_a_valid_value() {
        check_choices_read_back(SETTINGS);
        check_choices_read_back(ROOT_SETTINGS);
    }

    fn check_format_examples_are_accepted<S: Default>(table: &[SettingInfo<S>]) {
        for info in table {
            for example in info.format.iter().flat_map(|f| f.split(" · ")) {
                let mut s = S::default();
                let r = (info.set)(&mut s, example);
                assert!(r.is_ok(), "{} = {example:?}: {r:?}", info.key);
                assert!((info.get)(&s).is_some(), "{} = {example:?}", info.key);
            }
        }
    }

    #[test]
    fn every_format_example_is_accepted() {
        check_format_examples_are_accepted(SETTINGS);
        check_format_examples_are_accepted(ROOT_SETTINGS);
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
                Some("10m".to_string()),
                Some("nvim-tmux".to_string()),
                Some("none".to_string()),
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
        assert_eq!(
            texts,
            [Some("dark".to_string()), Some("/dotfiles/run".to_string())]
        );
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
            ("open_with", "my script"),
            ("on_create", "idea.py"),
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

    // ---------- root settings in the rows ----------

    fn labels(rows: &[EffectiveSetting]) -> Vec<&'static str> {
        rows.iter().map(|r| r.label).collect()
    }

    fn all_labels() -> Vec<&'static str> {
        SETTINGS
            .iter()
            .map(|i| i.label)
            .chain(ROOT_SETTINGS.iter().map(|i| i.label))
            .collect()
    }

    /// root: [top (task), uni: [lab (task)]], theme = "dark".
    fn root_tree() -> Tree {
        let mut t = tree_with(vec![task("top"), container("uni", vec![task("lab")])]);
        let root = t.get_mut(&[]).and_then(Node::as_container_mut).unwrap();
        root.root_settings.theme = Some("dark".into());
        t
    }

    #[test]
    fn root_row_lists_root_settings_after_the_others() {
        let t = root_tree();
        let rows = t.effective_settings(&[]);

        assert_eq!(labels(&rows), all_labels());
        let theme = rows.iter().find(|r| r.label == "theme").unwrap();
        assert_eq!(
            theme.value,
            Some(Resolved {
                value: "dark".into(),
                source: Source::Own,
            })
        );
    }

    #[test]
    fn task_in_the_root_lists_root_settings_too() {
        // its settings form is the root's, so the tab matches it
        assert_eq!(labels(&root_tree().effective_settings(&[0])), all_labels());
    }

    #[test]
    fn below_the_root_no_root_settings() {
        let t = root_tree();
        let expected: Vec<_> = SETTINGS.iter().map(|i| i.label).collect();
        assert_eq!(labels(&t.effective_settings(&[1])), expected); // uni
        assert_eq!(labels(&t.effective_settings(&[1, 0])), expected); // lab
    }

    #[test]
    fn unset_root_setting_has_no_value() {
        let t = tree_with(vec![]);
        let rows = t.effective_settings(&[]);
        let theme = rows.iter().find(|r| r.label == "theme").unwrap();
        assert_eq!(theme.value, None);
    }
}
