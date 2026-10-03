//! Container settings: what each container may set, and the built-in
//! defaults. Unset values are inherited (`resolve`: `Tree::setting`), and
//! only set values are saved.
//!
//! - `resolve`: `Tree::setting`, `Resolved`, `Source`
//! - `view`:    `SETTINGS` (every setting as text, for the UI) and
//!   `Tree::effective_settings`
//! - `run`:     `RunName`, `RunSetting` (`open_with`, `on_create`)

mod resolve;
mod run;
pub mod view;

use std::{fmt, path::PathBuf, str::FromStr};

use chrono::NaiveTime;
use serde::{Deserialize, Serialize};

pub use resolve::{Resolved, Source};
pub use run::{RunName, RunSetting};

use crate::model::time::{DeadlineRule, Minutes};

/// Settings any container can set, flattened into its `.udo.toml`. Every
/// field is optional: `None` = take it from the parent.
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContainerSettings {
    pub archive_dir: Option<PathBuf>,
    /// New tasks get a folder. Inherited; built-in: `none` (opt-in).
    pub task_folders: Option<TaskFolderSetting>,
    /// Due date of new tasks, e.g. `fri 22:00`. Inherited; built-in:
    /// `+1d 12:00` (tomorrow noon).
    pub default_deadline: Option<DeadlineRule>,
    /// First guess of how long a task takes, e.g. `1h30`. Inherited; no
    /// built-in default.
    pub estimate: Option<Minutes>,
    /// Run config for opening a node (`o`, `udo run`). Inherited; no
    /// built-in default; `none` switches it off.
    pub open_with: Option<RunSetting>,
    /// Run config after creating a task or container below. Inherited; no
    /// built-in default; `none` switches it off.
    pub on_create: Option<RunSetting>,
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

impl TaskFolderSetting {
    /// Every value, in the order the UI offers them.
    pub const ALL: [Self; 2] = [Self::Auto, Self::None];

    /// Labels of `ALL`, same order (the choices of the settings form).
    pub const LABELS: &[&str] = &[Self::ALL[0].label(), Self::ALL[1].label()];

    /// As written in the file; also what `Display` prints.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::None => "none",
        }
    }
}

impl fmt::Display for TaskFolderSetting {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

impl FromStr for TaskFolderSetting {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.trim();
        Self::ALL
            .into_iter()
            .find(|v| v.label() == s)
            .ok_or_else(|| format!("{s:?}: expected {}", Self::LABELS.join(" or ")))
    }
}

/// Settings that only exist once, on the root (`[root]` table of the root's
/// `.udo.toml`). Not inherited: read with `Tree::root_settings`.
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
pub struct RootSettings {
    pub theme: Option<String>,
    /// Where the run configs live (scripts, name = file stem). Unset:
    /// `<root>/run` (`Tree::run_dir`). Absolute; `~/…` is expanded when set
    /// (scripts in a dotfiles repo).
    pub run_dir: Option<PathBuf>,
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
        test_util::uni_tree,
    };

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
            folders(&uni_tree(), &[0, 0]),
            Some(Resolved {
                value: builtin.unwrap(),
                source: Source::Default,
            })
        );
    }

    #[test]
    fn task_folders_none_on_the_parent_is_inherited() {
        let mut t = uni_tree();
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
        let mut t = uni_tree();
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
    fn task_folders_displays_like_the_file() {
        for v in [TaskFolderSetting::Auto, TaskFolderSetting::None] {
            let s = ContainerSettings {
                task_folders: Some(v),
                ..Default::default()
            };
            let file = toml::to_string(&s).unwrap();
            assert_eq!(file.trim(), format!("task_folders = \"{v}\""));
        }
    }

    #[test]
    fn task_folders_rejects_custom() {
        // `custom` is a form choice (one task, typed path), never a setting
        assert!(toml::from_str::<ContainerSettings>("task_folders = \"custom\"").is_err());
        assert!("custom".parse::<TaskFolderSetting>().is_err());
    }

    #[test]
    fn task_folders_parses_what_it_displays() {
        for v in TaskFolderSetting::ALL {
            assert_eq!(v.to_string().parse(), Ok(v));
        }
    }

    #[test]
    fn task_folders_labels_match_all() {
        let labels: Vec<_> = TaskFolderSetting::ALL.iter().map(|v| v.label()).collect();
        assert_eq!(labels, TaskFolderSetting::LABELS);
    }

    // ---------- default_deadline ----------

    fn deadline(t: &Tree, path: &[usize]) -> Option<Resolved<DeadlineRule>> {
        t.setting(path, |s| s.default_deadline)
    }

    #[test]
    fn default_deadline_builtin_is_tomorrow_noon() {
        assert_eq!(
            deadline(&uni_tree(), &[0, 0]),
            Some(Resolved {
                value: "+1d 12:00".parse().unwrap(),
                source: Source::Default,
            })
        );
    }

    #[test]
    fn default_deadline_is_inherited() {
        let mut t = uni_tree();
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

    // ---------- open_with / on_create ----------

    fn open_with(t: &Tree, path: &[usize]) -> Option<Resolved<RunSetting>> {
        t.setting(path, |s| s.open_with.clone())
    }

    fn set_open_with(t: &mut Tree, path: &[usize], text: &str) {
        let c = t.get_mut(path).and_then(Node::as_container_mut).unwrap();
        c.settings.open_with = Some(text.parse().unwrap());
    }

    #[test]
    fn run_settings_have_no_builtin_default() {
        assert_eq!(open_with(&uni_tree(), &[0, 0]), None);
        assert_eq!(uni_tree().setting(&[0, 0], |s| s.on_create.clone()), None);
    }

    #[test]
    fn open_with_is_inherited() {
        let mut t = uni_tree();
        set_open_with(&mut t, &[0], "nvim-tmux");

        assert_eq!(
            open_with(&t, &[0, 0, 0]), // task "lab" in cs, below uni
            Some(Resolved {
                value: "nvim-tmux".parse().unwrap(),
                source: Source::Inherited(vec![0]),
            })
        );
    }

    #[test]
    fn none_on_a_child_switches_the_parents_off() {
        let mut t = uni_tree();
        set_open_with(&mut t, &[0], "nvim-tmux");
        set_open_with(&mut t, &[0, 0], "none");

        let resolved = open_with(&t, &[0, 0, 0]).unwrap();

        assert_eq!(
            (resolved.value, resolved.source),
            (RunSetting::Off, Source::Inherited(vec![0, 0]))
        );
    }

    /// `Tree::open_with`: just the script to run, whatever set it.
    #[test]
    fn open_with_names_the_script_or_nothing() {
        let mut t = uni_tree();
        assert_eq!(t.open_with(&[0, 0, 0]), None); // unset

        set_open_with(&mut t, &[0], "nvim-tmux");
        assert_eq!(t.open_with(&[0, 0, 0]), Some("nvim-tmux".parse().unwrap()));

        set_open_with(&mut t, &[0, 0], "none");
        assert_eq!(t.open_with(&[0, 0, 0]), None); // switched off
        assert_eq!(t.open_with(&[1]), None); // "work": not below uni
    }

    #[test]
    fn default_deadline_in_the_file() {
        let s: ContainerSettings = toml::from_str("default_deadline = \"+7d 23:59\"").unwrap();
        assert_eq!(s.default_deadline, Some("+7d 23:59".parse().unwrap()));
        assert_eq!(toml::to_string(&s).unwrap().trim(), "default_deadline = \"+7d 23:59\"");
    }
}
