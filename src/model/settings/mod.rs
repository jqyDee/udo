//! Container settings: what each container may set, and the built-in
//! defaults. Unset values are inherited (`resolve`: `Tree::setting`), and
//! only set values are saved.
//!
//! - `resolve`: `Tree::setting`, `Resolved`, `Source`

mod resolve;

use std::path::PathBuf;

use chrono::NaiveTime;
use serde::{Deserialize, Serialize};

pub use resolve::{Resolved, Source};

use crate::model::time::DeadlineRule;

/// Settings any container can set, flattened into its `.udo.toml`. Every
/// field is optional: `None` = take it from the parent.
#[derive(Default, Clone, Serialize, Deserialize)]
pub struct ContainerSettings {
    pub archive_dir: Option<PathBuf>,
    /// New tasks get a folder. Inherited; built-in: `none` (opt-in).
    pub task_folders: Option<TaskFolderSetting>,
    /// Due date of new tasks, e.g. `fri 22:00`. Inherited; built-in:
    /// `+1d 12:00` (tomorrow noon).
    pub default_deadline: Option<DeadlineRule>,
}

impl ContainerSettings {
    /// Values used when no container sets them. `None` = no default.
    ///
    /// `task_folders` is `none` on purpose: task folders are opt-in, set
    /// `auto` on a container to get them for everything below it.
    pub fn builtin() -> Self {
        Self {
            task_folders: Some(TaskFolderSetting::None),
            default_deadline: Some(DeadlineRule::InDays {
                days: 1,
                time: NaiveTime::from_hms_opt(12, 0, 0).expect("12:00 is a valid time"),
            }),
            ..Self::default()
        }
    }
}

/// Whether new tasks get their own folder (`<container dir>/<task name>`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TaskFolderSetting {
    Auto,
    None,
}

/// Settings that only exist once, on the root (`[root]` table of the root's
/// `.udo.toml`). Not inherited: read with `Tree::root_settings`.
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
pub struct RootSettings {
    pub theme: Option<String>,
}

impl RootSettings {
    /// Nothing set: the `[root]` table is left out of the file.
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        model::{node::Node, tree::Tree},
        test_util::{container, task, tree_with},
    };

    /// root
    /// ├─ uni          [0]
    /// │  └─ cs        [0,0]
    /// │     └─ lab    [0,0,0]  (task)
    /// └─ work         [1]
    fn tree() -> Tree {
        tree_with(vec![
            container("uni", vec![container("cs", vec![task("lab")])]),
            container("work", vec![]),
        ])
    }

    // ---------- task_folders ----------

    fn set_folders(t: &mut Tree, path: &[usize], value: TaskFolderSetting) {
        let c = t.get_mut(path).and_then(Node::as_container_mut).unwrap();
        c.settings.task_folders = Some(value);
    }

    fn folders(t: &Tree, path: &[usize]) -> Option<Resolved<TaskFolderSetting>> {
        t.setting(path, |s| s.task_folders)
    }

    #[test]
    fn task_folders_falls_back_to_the_builtin_default() {
        let builtin = ContainerSettings::builtin().task_folders;
        assert!(builtin.is_some(), "task_folders needs a built-in default");

        assert_eq!(
            folders(&tree(), &[0, 0]),
            Some(Resolved {
                value: builtin.unwrap(),
                source: Source::Default,
            })
        );
    }

    #[test]
    fn task_folders_none_on_the_parent_is_inherited() {
        let mut t = tree();
        set_folders(&mut t, &[0], TaskFolderSetting::None);

        assert_eq!(
            folders(&t, &[0, 0]),
            Some(Resolved {
                value: TaskFolderSetting::None,
                source: Source::Inherited(vec![0]),
            })
        );
    }

    #[test]
    fn task_folders_child_switches_back_on() {
        let mut t = tree();
        set_folders(&mut t, &[0], TaskFolderSetting::None);
        set_folders(&mut t, &[0, 0], TaskFolderSetting::Auto);

        assert_eq!(
            folders(&t, &[0, 0]),
            Some(Resolved {
                value: TaskFolderSetting::Auto,
                source: Source::Own,
            })
        );
    }

    #[test]
    fn task_folders_is_written_lowercase() {
        let s = ContainerSettings {
            task_folders: Some(TaskFolderSetting::None),
            ..Default::default()
        };
        let text = toml::to_string(&s).unwrap();
        assert_eq!(text.trim(), "task_folders = \"none\"");

        let back: ContainerSettings = toml::from_str("task_folders = \"auto\"").unwrap();
        assert_eq!(back.task_folders, Some(TaskFolderSetting::Auto));
    }

    #[test]
    fn task_folders_rejects_custom() {
        // `custom` is a form choice (one task, typed path), never a setting
        assert!(toml::from_str::<ContainerSettings>("task_folders = \"custom\"").is_err());
    }

    // ---------- default_deadline ----------

    fn deadline(t: &Tree, path: &[usize]) -> Option<Resolved<DeadlineRule>> {
        t.setting(path, |s| s.default_deadline)
    }

    #[test]
    fn default_deadline_builtin_is_tomorrow_noon() {
        assert_eq!(
            deadline(&tree(), &[0, 0]),
            Some(Resolved {
                value: "+1d 12:00".parse().unwrap(),
                source: Source::Default,
            })
        );
    }

    #[test]
    fn default_deadline_is_inherited() {
        let mut t = tree();
        let uni = t.get_mut(&[0]).and_then(Node::as_container_mut).unwrap();
        uni.settings.default_deadline = Some("fri 22:00".parse().unwrap());

        assert_eq!(
            deadline(&t, &[0, 0, 0]), // task "lab" in cs, below uni
            Some(Resolved {
                value: "fri 22:00".parse().unwrap(),
                source: Source::Inherited(vec![0]),
            })
        );
        assert_eq!(deadline(&t, &[1]).unwrap().source, Source::Default); // "work"
    }

    #[test]
    fn default_deadline_in_the_file() {
        let s: ContainerSettings = toml::from_str("default_deadline = \"+7d 23:59\"").unwrap();
        assert_eq!(s.default_deadline, Some("+7d 23:59".parse().unwrap()));
        assert_eq!(
            toml::to_string(&s).unwrap().trim(),
            "default_deadline = \"+7d 23:59\""
        );
    }
}
