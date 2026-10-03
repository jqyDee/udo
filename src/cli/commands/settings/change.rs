//! `udo settings set [NODE] KEY=VALUE…` and `udo settings unset [NODE]
//! KEY…`: one code path, unset = a blank value.

use std::{fmt, path::Path};

use serde::Serialize;

use super::{SettingsAction, container_of};
use crate::{
    Res,
    cli::{
        report::Report,
        resolve::{path_text, resolve},
    },
    core::Core,
    model::settings::{
        ContainerSettings, RootSettings,
        view::{ROOT_SETTINGS, SETTINGS},
    },
};

/// What `set` / `unset` saved.
#[derive(Serialize)]
pub struct SettingsSaved {
    pub path: String,
    pub keys: Vec<String>,
}

/// `set` (`KEY=VALUE`) or `unset` (`KEY`): apply every value, then save
/// once. One bad key or value and nothing changes.
pub async fn run(core: &mut Core, cwd: &Path, action: &SettingsAction) -> Res<SettingsSaved> {
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

impl fmt::Display for SettingsSaved {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "saved settings of {}: {}",
            self.path,
            self.keys.join(", ")
        )
    }
}

impl Report for SettingsSaved {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        cli::commands::settings::show::{self, tests::row},
        model::tree::Tree,
        test_util::core, // disk_tree, root: [a, ws (tmp/ws): [b]]
    };

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

    #[tokio::test]
    async fn set_saves_all_values_at_once() {
        let (tmp, mut core) = core().await;
        let action = set(&["ws", "estimate=1h30", "default_deadline=fri 22:00"]);

        let saved = run(&mut core, tmp.path(), &action).await.unwrap();

        assert_eq!(
            saved.to_string(),
            "saved settings of ws: estimate, default_deadline"
        );
        let reloaded = Tree::load_from(tmp.path()).await.unwrap();
        let ws = reloaded.get(&[1]).and_then(|n| n.as_container()).unwrap();
        assert_eq!(
            ws.settings.estimate.map(|m| m.to_string()).as_deref(),
            Some("1h30")
        );
        assert!(ws.settings.default_deadline.is_some());
    }

    /// No node given: the first value already has `=`, so the node is the
    /// one of the current folder.
    #[tokio::test]
    async fn set_without_a_node_takes_the_current_folder() {
        let (tmp, mut core) = core().await;

        let saved = run(&mut core, &tmp.path().join("ws"), &set(&["estimate=45m"]))
            .await
            .unwrap();

        assert_eq!(saved.path, "ws");
    }

    #[tokio::test]
    async fn one_bad_value_changes_nothing() {
        let (tmp, mut core) = core().await;
        let action = set(&["ws", "estimate=1h30", "default_deadline=someday"]);

        let err = run(&mut core, tmp.path(), &action).await.err().unwrap();

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

        let err = run(&mut core, tmp.path(), &set(&["ws", "colour=red"]))
            .await
            .err()
            .unwrap();

        assert!(err.to_string().contains("known: task_folders"), "{err}");
    }

    #[tokio::test]
    async fn root_settings_only_on_the_root() {
        let (tmp, mut core) = core().await;

        let on_ws = run(&mut core, tmp.path(), &set(&["ws", "theme=dark"])).await;
        let on_root = run(&mut core, tmp.path(), &set(&["/", "theme=dark"])).await;

        assert!(on_ws.err().unwrap().to_string().contains("only the root"));
        assert!(on_root.is_ok());
        assert_eq!(core.tree().root_settings().theme.as_deref(), Some("dark"));
    }

    #[tokio::test]
    async fn unset_inherits_again() {
        let (tmp, mut core) = core().await;
        run(&mut core, tmp.path(), &set(&["ws", "estimate=1h30"]))
            .await
            .unwrap();

        let saved = run(&mut core, tmp.path(), &unset(&["ws", "estimate"]))
            .await
            .unwrap();

        assert_eq!(saved.keys, vec!["estimate"]);
        let view = show::run(&core, tmp.path(), Some("ws")).unwrap();
        assert_ne!(row(&view, "estimate").source.as_deref(), Some("own"));
    }

    #[tokio::test]
    async fn a_value_without_equals_is_an_error() {
        let (tmp, mut core) = core().await;

        let err = run(&mut core, tmp.path(), &set(&["ws", "estimate=1h", "oops"]))
            .await
            .err()
            .unwrap();

        assert!(err.to_string().contains("expected KEY=VALUE"), "{err}");
    }
}
