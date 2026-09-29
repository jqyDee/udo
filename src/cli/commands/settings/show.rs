//! `udo settings [NODE]`: every setting with its value and source.

use std::{fmt, path::Path};

use serde::Serialize;

use super::container_of;
use crate::{
    Res,
    cli::{
        report::Report,
        resolve::{path_text, resolve},
    },
    core::Core,
    model::settings::view::{ROOT_SETTINGS, SETTINGS},
};

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

/// The settings of `node`'s container.
pub fn run(core: &Core, cwd: &Path, node: Option<&str>) -> Res<SettingsView> {
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

impl Report for SettingsView {}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use crate::{
        cli::commands::settings::{SettingsAction, change},
        test_util::core, // disk_tree, root: [a, ws (tmp/ws): [b]]
    };

    pub(in crate::cli::commands::settings) fn row<'v>(
        view: &'v SettingsView,
        key: &str,
    ) -> &'v SettingRow {
        view.rows.iter().find(|r| r.key == key).unwrap()
    }

    #[tokio::test]
    async fn show_lists_every_setting_with_its_source() {
        let (tmp, mut core) = core().await;
        let set = SettingsAction::Set {
            args: vec!["/".into(), "estimate=1h30".into()],
        };
        change::run(&mut core, tmp.path(), &set).await.unwrap();

        let view = run(&core, tmp.path(), Some("ws")).unwrap();

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

        let view = run(&core, tmp.path(), Some("/")).unwrap();

        assert_eq!(view.rows.last().unwrap().key, "theme");
    }

    #[tokio::test]
    async fn a_task_shows_its_containers_settings() {
        let (tmp, core) = core().await;

        let view = run(&core, tmp.path(), Some("b")).unwrap();

        assert_eq!(view.path, "ws");
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
}
