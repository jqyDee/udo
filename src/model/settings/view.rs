use crate::model::{
    settings::{ContainerSettings, Resolved},
    tree::Tree,
};

/// One setting as the UI sees it: its name and its value as text.
pub struct SettingInfo {
    /// Key in `.udo.toml`.
    pub key: &'static str,
    /// Shown in the UI.
    pub label: &'static str,
    /// This container's own value as text (None = not set here).
    pub get: fn(&ContainerSettings) -> Option<String>,
}

/// Every container setting, in the order the UI shows them.
pub const SETTINGS: &[SettingInfo] = &[
    SettingInfo {
        key: "task_folders",
        label: "task folders",
        get: |s| s.task_folders.map(|v| v.to_string()),
    },
    SettingInfo {
        key: "default_deadline",
        label: "deadline",
        get: |s| s.default_deadline.map(|v| v.to_string()),
    },
    SettingInfo {
        key: "archive_dir",
        label: "archive",
        get: |s| s.archive_dir.as_ref().map(|p| p.display().to_string()),
    },
];

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
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

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

    #[test]
    fn every_setting_key_is_a_field() {
        let text = toml::to_string(&all_set()).unwrap();
        for info in SETTINGS {
            assert!(
                text.contains(&format!("{} = ", info.key)),
                "{} not in:\n{text}",
                info.key
            );
        }
        // and nothing in the file is missing from the table
        assert_eq!(text.lines().count(), SETTINGS.len(), "got:\n{text}");
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
