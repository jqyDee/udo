//! `udo settings [NODE]`, `udo settings set [NODE] KEY=VALUE…`,
//! `udo settings unset [NODE] KEY…`. Settings belong to containers; on a
//! task they are its container's (like the TUI's settings tab). Values are
//! parsed by the same `SETTINGS` / `ROOT_SETTINGS` table as the TUI form.

use std::{fmt, path::Path};

use serde::Serialize;

use super::{
    report::Report,
    resolve::{path_text, resolve},
};
use crate::{
    Res,
    core::Core,
    model::{
        NodePath,
        settings::{
            ContainerSettings, RootSettings,
            view::{ROOT_SETTINGS, SETTINGS},
        },
    },
};

#[derive(clap::Args)]
#[command(args_conflicts_with_subcommands = true)]
pub struct SettingsArgs {
    #[command(subcommand)]
    pub action: Option<SettingsAction>,
    /// Default: the node of the current folder (a task: its container)
    pub node: Option<String>,
}

#[derive(clap::Subcommand)]
pub enum SettingsAction {
    /// Set values, e.g. `udo settings set uni estimate=1h30`; the node can
    /// be left out (the current folder)
    Set {
        /// [NODE] KEY=VALUE...
        #[arg(required = true, num_args = 1.., value_name = "KEY=VALUE")]
        args: Vec<String>,
    },
    /// Unset values, so they are inherited again, e.g. `udo settings unset
    /// uni estimate`; the node can be left out (the current folder)
    Unset {
        /// [NODE] KEY...
        #[arg(required = true, num_args = 1.., value_name = "KEY")]
        args: Vec<String>,
    },
}

/// Every setting of one container: its value and where it comes from.
#[derive(Serialize)]
pub struct SettingsView {
    pub path: String,
    pub rows: Vec<SettingRow>,
}

/// One setting. `value` None: set nowhere and no default.
#[derive(Serialize)]
pub struct SettingRow {
    pub key: String,
    pub value: Option<String>,
    /// `own`, `from uni`, `default`; None without a value.
    pub source: Option<String>,
}

/// What `set` / `unset` saved.
#[derive(Serialize)]
pub struct SettingsSaved {
    pub path: String,
    pub keys: Vec<String>,
}

/// The settings of `node`'s container.
pub fn show(core: &Core, cwd: &Path, node: Option<&str>) -> Res<SettingsView> {
    let tree = core.tree();
    let path = container_of(core, resolve(tree, node, cwd)?)?;
    let mut rows: Vec<SettingRow> = SETTINGS
        .iter()
        .map(|info| {
            let resolved = tree.setting(&path, info.get);
            SettingRow {
                key: info.key.into(),
                source: resolved.as_ref().map(|r| tree.source_text(&r.source)),
                value: resolved.map(|r| r.value),
            }
        })
        .collect();
    if path.is_empty() {
        // root-only settings: own or unset, never inherited
        rows.extend(ROOT_SETTINGS.iter().map(|info| {
            let value = (info.get)(tree.root_settings());
            SettingRow {
                key: info.key.into(),
                source: value.as_ref().map(|_| "own".into()),
                value,
            }
        }));
    }
    Ok(SettingsView {
        path: path_text(tree, &path),
        rows,
    })
}

/// `set` (`KEY=VALUE`) or `unset` (`KEY`): apply every value, then save
/// once. One bad key or value and nothing changes.
pub async fn change(core: &mut Core, cwd: &Path, action: &SettingsAction) -> Res<SettingsSaved> {
    let (args, unset) = match action {
        SettingsAction::Set { args } => (args, false),
        SettingsAction::Unset { args } => (args, true),
    };
    // the first argument is the node unless it is already a value
    let (node, values) = match args.split_first() {
        Some((first, rest)) if !is_value(first, unset) => (Some(first.as_str()), rest),
        _ => (None, args.as_slice()),
    };
    if values.is_empty() {
        return Err(if unset {
            "which settings? give KEY..."
        } else {
            "which settings? give KEY=VALUE..."
        }
        .into());
    }
    let path = container_of(core, resolve(core.tree(), node, cwd)?)?;
    let container = core
        .tree()
        .get(&path)
        .and_then(|n| n.as_container())
        .ok_or("not a container")?;
    let mut settings = container.settings.clone();
    let mut root = path.is_empty().then(|| container.root_settings.clone());

    let mut keys = vec![];
    for value in values {
        let (key, text) = if unset {
            (value.as_str(), "") // blank = unset
        } else {
            value
                .split_once('=')
                .ok_or_else(|| format!("expected KEY=VALUE, got {value:?}"))?
        };
        apply(&mut settings, root.as_mut(), key.trim(), text).map_err(|e| format!("{key}: {e}"))?;
        keys.push(key.trim().to_string());
    }
    core.set_settings(&path, settings, root).await?;
    Ok(SettingsSaved {
        path: path_text(core.tree(), &path),
        keys,
    })
}

/// Whether `arg` is a value (not the node): `KEY=VALUE` for `set`, a known
/// key for `unset`.
fn is_value(arg: &str, unset: bool) -> bool {
    if unset {
        is_key(arg.trim())
    } else {
        arg.contains('=')
    }
}

fn is_key(key: &str) -> bool {
    SETTINGS.iter().any(|i| i.key == key) || ROOT_SETTINGS.iter().any(|i| i.key == key)
}

/// Parse `text` into setting `key` (blank = unset), with the same code as
/// the TUI form. `root`: the `[root]` settings, only on the root.
fn apply(
    settings: &mut ContainerSettings,
    root: Option<&mut RootSettings>,
    key: &str,
    text: &str,
) -> Result<(), String> {
    if let Some(info) = SETTINGS.iter().find(|i| i.key == key) {
        return (info.set)(settings, text);
    }
    if let Some(info) = ROOT_SETTINGS.iter().find(|i| i.key == key) {
        return match root {
            Some(root) => (info.set)(root, text),
            None => Err("only the root has this setting (use `/`)".into()),
        };
    }
    let known: Vec<&str> = SETTINGS
        .iter()
        .map(|i| i.key)
        .chain(ROOT_SETTINGS.iter().map(|i| i.key))
        .collect();
    Err(format!("unknown setting, known: {}", known.join(", ")))
}

/// The container a node's settings live in: itself, or a task's container.
fn container_of(core: &Core, path: NodePath) -> Res<NodePath> {
    Ok(core
        .tree()
        .nearest_file_owner(&path)
        .ok_or("no such node")?)
}

impl fmt::Display for SettingsView {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let width = self.rows.iter().map(|r| r.key.len()).max().unwrap_or(0) + 2;
        write!(f, "settings of {}", self.path)?;
        for row in &self.rows {
            match (&row.value, &row.source) {
                (Some(value), Some(source)) => {
                    write!(f, "\n{:<width$}{value:<14}{source}", row.key)?
                }
                _ => write!(f, "\n{:<width$}-", row.key)?,
            }
        }
        Ok(())
    }
}

impl fmt::Display for SettingsSaved {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "saved settings of {}: {}", self.path, self.keys.join(", "))
    }
}

impl Report for SettingsView {}
impl Report for SettingsSaved {}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::*;
    use crate::{cli::Cli, model::tree::Tree, storage::Storage, test_util::disk_tree};

    async fn core() -> (tempfile::TempDir, Core) {
        let (tmp, tree) = disk_tree().await; // root: [a, ws (tmp/ws): [b]]
        (tmp, Core::new(tree, Storage::in_memory()))
    }

    fn set(args: &[&str]) -> SettingsAction {
        SettingsAction::Set {
            args: args.iter().map(|a| a.to_string()).collect(),
        }
    }

    fn unset(args: &[&str]) -> SettingsAction {
        SettingsAction::Unset {
            args: args.iter().map(|a| a.to_string()).collect(),
        }
    }

    fn row<'v>(view: &'v SettingsView, key: &str) -> &'v SettingRow {
        view.rows.iter().find(|r| r.key == key).unwrap()
    }

    #[tokio::test]
    async fn show_lists_every_setting_with_its_source() {
        let (tmp, mut core) = core().await;
        change(&mut core, tmp.path(), &set(&["/", "estimate=1h30"]))
            .await
            .unwrap();

        let view = show(&core, tmp.path(), Some("ws")).unwrap();

        let keys: Vec<&str> = view.rows.iter().map(|r| r.key.as_str()).collect();
        assert_eq!(
            keys,
            vec![
                "task_folders",
                "default_deadline",
                "archive_dir",
                "estimate"
            ]
        );
        let estimate = row(&view, "estimate");
        assert_eq!(estimate.value.as_deref(), Some("1h30"));
        assert_eq!(estimate.source.as_deref(), Some("from root"));
        assert_eq!(row(&view, "task_folders").source.as_deref(), Some("default"));
        assert_eq!(row(&view, "archive_dir").value, None);
    }

    #[tokio::test]
    async fn the_root_shows_its_own_settings_too() {
        let (tmp, core) = core().await;

        let view = show(&core, tmp.path(), Some("/")).unwrap();

        assert_eq!(view.rows.last().unwrap().key, "theme");
    }

    #[tokio::test]
    async fn a_task_shows_its_containers_settings() {
        let (tmp, core) = core().await;

        let view = show(&core, tmp.path(), Some("b")).unwrap();

        assert_eq!(view.path, "ws");
    }

    #[tokio::test]
    async fn set_saves_all_values_at_once() {
        let (tmp, mut core) = core().await;
        let action = set(&["ws", "estimate=1h30", "default_deadline=fri 22:00"]);

        let saved = change(&mut core, tmp.path(), &action).await.unwrap();

        assert_eq!(saved.to_string(), "saved settings of ws: estimate, default_deadline");
        let reloaded = Tree::load_from(tmp.path()).await.unwrap();
        let ws = reloaded.get(&[1]).and_then(|n| n.as_container()).unwrap();
        assert_eq!(ws.settings.estimate.map(|m| m.to_string()).as_deref(), Some("1h30"));
        assert!(ws.settings.default_deadline.is_some());
    }

    /// No node given: the first value already has `=`, so the node is the
    /// one of the current folder.
    #[tokio::test]
    async fn set_without_a_node_takes_the_current_folder() {
        let (tmp, mut core) = core().await;

        let saved = change(&mut core, &tmp.path().join("ws"), &set(&["estimate=45m"]))
            .await
            .unwrap();

        assert_eq!(saved.path, "ws");
    }

    #[tokio::test]
    async fn one_bad_value_changes_nothing() {
        let (tmp, mut core) = core().await;
        let action = set(&["ws", "estimate=1h30", "default_deadline=someday"]);

        let err = change(&mut core, tmp.path(), &action).await.err().unwrap();

        assert!(err.to_string().starts_with("default_deadline: "), "{err}");
        let ws = core
            .tree()
            .get(&[1])
            .and_then(|n| n.as_container())
            .unwrap();
        assert_eq!(ws.settings.estimate, None);
    }

    #[tokio::test]
    async fn an_unknown_key_names_the_known_ones() {
        let (tmp, mut core) = core().await;

        let err = change(&mut core, tmp.path(), &set(&["ws", "colour=red"]))
            .await
            .err()
            .unwrap();

        assert!(err.to_string().contains("known: task_folders"), "{err}");
    }

    #[tokio::test]
    async fn root_settings_only_on_the_root() {
        let (tmp, mut core) = core().await;

        let on_ws = change(&mut core, tmp.path(), &set(&["ws", "theme=dark"])).await;
        let on_root = change(&mut core, tmp.path(), &set(&["/", "theme=dark"])).await;

        assert!(on_ws.err().unwrap().to_string().contains("only the root"));
        assert!(on_root.is_ok());
        assert_eq!(core.tree().root_settings().theme.as_deref(), Some("dark"));
    }

    #[tokio::test]
    async fn unset_inherits_again() {
        let (tmp, mut core) = core().await;
        change(&mut core, tmp.path(), &set(&["ws", "estimate=1h30"]))
            .await
            .unwrap();

        let saved = change(&mut core, tmp.path(), &unset(&["ws", "estimate"]))
            .await
            .unwrap();

        assert_eq!(saved.keys, vec!["estimate"]);
        let view = show(&core, tmp.path(), Some("ws")).unwrap();
        assert_ne!(row(&view, "estimate").source.as_deref(), Some("own"));
    }

    #[tokio::test]
    async fn a_value_without_equals_is_an_error() {
        let (tmp, mut core) = core().await;

        let err = change(&mut core, tmp.path(), &set(&["ws", "estimate=1h", "oops"]))
            .await
            .err()
            .unwrap();

        assert!(err.to_string().contains("expected KEY=VALUE"), "{err}");
    }

    #[test]
    fn text_has_one_aligned_row_per_setting() {
        let view = SettingsView {
            path: "ws".into(),
            rows: vec![
                SettingRow {
                    key: "estimate".into(),
                    value: Some("1h30".into()),
                    source: Some("own".into()),
                },
                SettingRow {
                    key: "archive_dir".into(),
                    value: None,
                    source: None,
                },
            ],
        };

        assert_eq!(
            view.to_string(),
            "settings of ws\nestimate     1h30          own\narchive_dir  -"
        );
    }

    #[test]
    fn show_and_set_parse() {
        assert!(Cli::try_parse_from(["udo", "settings"]).is_ok());
        assert!(Cli::try_parse_from(["udo", "settings", "uni"]).is_ok());
        assert!(Cli::try_parse_from(["udo", "settings", "set", "uni", "estimate=1h"]).is_ok());
        assert!(Cli::try_parse_from(["udo", "settings", "set"]).is_err());
    }
}
