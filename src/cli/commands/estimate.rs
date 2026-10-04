//! `udo estimate [NODE]`: what udo estimates for a task (or a typical task
//! in a container), and where that comes from: how many tasks it learned
//! from, and the prior it started from.

use std::{fmt, path::Path};

use serde::Serialize;

use crate::{
    Res,
    cli::{
        report::Report,
        resolve::{path_text, resolve},
    },
    core::Core,
    estimate::{Basis, Prior},
    model::{
        id::NodeId,
        time::{Minutes, Time},
        tree::Tree,
    },
};

#[derive(clap::Args)]
pub struct EstimateArgs {
    /// Default: the node of the current folder
    pub node: Option<String>,
}

/// One node's estimate. `minutes` `None`: nothing to estimate from.
#[derive(Serialize)]
pub struct Estimated {
    pub id: NodeId,
    pub path: String,
    /// A container: "a typical task in it"; a task: its container's
    /// estimate, without its own time.
    pub container: bool,
    pub minutes: Option<u32>,
    /// Tasks learned from; 0: only the prior.
    pub tasks: usize,
    pub prior: Option<PriorOut>,
}

/// Where the blend started.
#[derive(Serialize)]
#[serde(tag = "source", rename_all = "snake_case")]
pub enum PriorOut {
    /// The `estimate` setting of `from`.
    Setting { from: String, minutes: u32 },
    /// `tasks` tasks pooled in `from` (without the asked node's subtree).
    Parent {
        from: String,
        minutes: u32,
        tasks: usize,
    },
}

/// The estimate of `args.node`, sessions timed up to `now`.
pub async fn run(core: &Core, cwd: &Path, now: Time, args: &EstimateArgs) -> Res<Estimated> {
    let tree = core.tree();
    let path = resolve(tree, args.node.as_deref(), cwd)?;
    let node = tree.get(&path).ok_or("no such node")?;
    let estimate = core.estimate(&path, now).await?;

    let (tasks, prior) = match estimate.map(|e| e.basis) {
        Some(Basis::Learned { tasks, prior, .. }) => (tasks, prior),
        Some(Basis::Prior(p)) => (0, Some(p)),
        None => (0, None),
    };
    Ok(Estimated {
        id: node.id(),
        path: path_text(tree, &path),
        container: node.as_container().is_some(),
        minutes: estimate.map(|e| e.minutes.get()),
        tasks,
        prior: prior.map(|p| prior_out(tree, p)),
    })
}

/// `p` with paths instead of ids.
fn prior_out(tree: &Tree, p: Prior) -> PriorOut {
    let from = |id| tree.path_of(id).map_or("?".into(), |p| path_text(tree, &p));
    match p {
        Prior::Setting { container, minutes } => PriorOut::Setting {
            from: from(container),
            minutes: minutes.get(),
        },
        Prior::Parent {
            container,
            tasks,
            minutes,
        } => PriorOut::Parent {
            from: from(container),
            minutes: minutes.get(),
            tasks,
        },
    }
}

/// Three lines: the estimate, what it learned from, the prior. Nothing to
/// estimate from: one line that says how to set a starting value.
impl fmt::Display for Estimated {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Some(minutes) = self.minutes else {
            return write!(
                f,
                "no estimate yet for {}: set one with udo settings set <container> estimate=1h30",
                self.path
            );
        };
        let head = if self.container {
            format!("{} for a task in {}", Minutes::new(minutes), self.path)
        } else {
            format!("{} for {}", Minutes::new(minutes), self.path)
        };
        write!(f, "{head}")?;
        match self.tasks {
            0 => write!(f, "\n  no tasks with tracked time here yet")?,
            1 => write!(f, "\n  learned from 1 task")?,
            n => write!(f, "\n  learned from {n} tasks")?,
        }
        match &self.prior {
            Some(PriorOut::Setting { from, minutes }) => {
                write!(f, "\n  prior: {}, set on {from}", Minutes::new(*minutes))
            }
            Some(PriorOut::Parent {
                from,
                minutes,
                tasks,
            }) => {
                write!(
                    f,
                    "\n  prior: {} from {tasks} tasks in {from}",
                    Minutes::new(*minutes)
                )
            }
            None => write!(f, "\n  no prior: nothing set above"),
        }
    }
}

impl Report for Estimated {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        cli::report::render,
        model::settings::ContainerSettings,
        test_util::{at, core, task}, // core: disk_tree, root: [a, ws: [b]]
    };

    fn named(node: &str) -> EstimateArgs {
        EstimateArgs {
            node: Some(node.into()),
        }
    }

    /// `b` (in ws) done with `minutes` of tracked time.
    async fn b_done_with(core: &mut Core, minutes: i64) {
        let end = at(9, 0) + chrono::TimeDelta::minutes(minutes);
        core.add_session(&[1, 0], at(9, 0), end, at(12, 0))
            .await
            .unwrap();
        core.set_done(&[1, 0], true, at(11, 0)).await.unwrap();
    }

    /// ws's own `estimate` setting.
    async fn set_ws_estimate(core: &mut Core, minutes: u32) {
        let settings = ContainerSettings {
            estimate: Some(Minutes::new(minutes)),
            ..Default::default()
        };
        core.set_settings(&[1], settings, None).await.unwrap();
    }

    /// An `Estimated` for the text tests: `minutes` from `tasks`, `prior`.
    fn estimated(tasks: usize, prior: Option<PriorOut>) -> Estimated {
        Estimated {
            id: NodeId::new(),
            path: "uni/cs".into(),
            container: true,
            minutes: Some(80),
            tasks,
            prior,
        }
    }

    #[tokio::test]
    async fn a_task_learns_from_a_done_task() {
        let (tmp, mut core) = core().await;
        b_done_with(&mut core, 60).await;
        core.create(&[1], task("c")).await.unwrap();

        let shown = run(&core, tmp.path(), at(12, 0), &named("c"))
            .await
            .unwrap();

        assert_eq!(shown.path, "ws/c");
        assert!(!shown.container);
        assert_eq!((shown.minutes, shown.tasks), (Some(60), 1));
        assert!(shown.prior.is_none());
    }

    #[tokio::test]
    async fn a_setting_is_the_prior() {
        let (tmp, mut core) = core().await;
        set_ws_estimate(&mut core, 90).await;

        let shown = run(&core, tmp.path(), at(12, 0), &named("ws"))
            .await
            .unwrap();

        assert!(shown.container);
        assert_eq!((shown.minutes, shown.tasks), (Some(90), 0));
        assert!(matches!(
            &shown.prior,
            Some(PriorOut::Setting { from, minutes: 90 }) if from == "ws"
        ));
    }

    #[tokio::test]
    async fn nothing_to_estimate_from() {
        let (tmp, core) = core().await;

        let shown = run(&core, tmp.path(), at(12, 0), &named("a"))
            .await
            .unwrap();

        assert_eq!(shown.minutes, None);
        assert_eq!(
            render(&shown, false).unwrap(),
            "no estimate yet for a: set one with udo settings set <container> estimate=1h30"
        );
    }

    #[tokio::test]
    async fn text_has_the_estimate_what_it_learned_from_and_the_prior() {
        let (tmp, mut core) = core().await;
        set_ws_estimate(&mut core, 90).await;
        b_done_with(&mut core, 70).await;

        let shown = run(&core, tmp.path(), at(12, 0), &named("ws"))
            .await
            .unwrap();

        // (3·90 + 70) / 4
        assert_eq!(
            render(&shown, false).unwrap(),
            "1h25 for a task in ws\n  learned from 1 task\n  prior: 1h30, set on ws"
        );
    }

    #[tokio::test]
    async fn a_task_names_itself_not_its_container() {
        let (tmp, mut core) = core().await;
        set_ws_estimate(&mut core, 90).await;

        let shown = run(&core, tmp.path(), at(12, 0), &named("b"))
            .await
            .unwrap();

        assert!(
            render(&shown, false)
                .unwrap()
                .starts_with("1h30 for ws/b\n")
        );
    }

    #[tokio::test]
    async fn json_is_flat_for_scripts() {
        let (tmp, mut core) = core().await;
        set_ws_estimate(&mut core, 90).await;

        let shown = run(&core, tmp.path(), at(12, 0), &named("ws"))
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_str(&render(&shown, true).unwrap()).unwrap();

        assert_eq!(json["path"], "ws");
        assert_eq!(json["container"], true);
        assert_eq!(json["minutes"], 90);
        assert_eq!(json["tasks"], 0);
        assert_eq!(json["prior"]["source"], "setting");
        assert_eq!(json["prior"]["from"], "ws");
        assert_eq!(json["prior"]["minutes"], 90);
    }

    #[tokio::test]
    async fn json_without_an_estimate_has_nulls() {
        let (tmp, core) = core().await;

        let shown = run(&core, tmp.path(), at(12, 0), &named("a"))
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_str(&render(&shown, true).unwrap()).unwrap();

        assert!(json["minutes"].is_null());
        assert!(json["prior"].is_null());
    }

    #[tokio::test]
    async fn default_node_is_the_current_folder() {
        let (tmp, core) = core().await;
        let args = EstimateArgs { node: None };

        let shown = run(&core, &tmp.path().join("ws"), at(12, 0), &args)
            .await
            .unwrap();

        assert_eq!(shown.path, "ws");
    }

    #[tokio::test]
    async fn unknown_node_is_an_error() {
        let (tmp, core) = core().await;

        assert!(
            run(&core, tmp.path(), at(12, 0), &named("nope"))
                .await
                .is_err()
        );
    }

    // ---------- text forms (no Core) ----------

    #[test]
    fn text_counts_several_tasks() {
        let text = estimated(6, None).to_string();

        assert_eq!(
            text,
            "1h20 for a task in uni/cs\n  learned from 6 tasks\n  no prior: nothing set above"
        );
    }

    #[test]
    fn text_names_a_pooled_prior() {
        let prior = PriorOut::Parent {
            from: "uni".into(),
            minutes: 105,
            tasks: 12,
        };

        let text = estimated(0, Some(prior)).to_string();

        assert_eq!(
            text,
            "1h20 for a task in uni/cs\n  no tasks with tracked time here yet\n  prior: 1h45 from 12 tasks in uni"
        );
    }

    #[test]
    fn json_names_a_pooled_prior() {
        let prior = PriorOut::Parent {
            from: "uni".into(),
            minutes: 105,
            tasks: 12,
        };

        let json = serde_json::to_value(estimated(0, Some(prior))).unwrap();

        assert_eq!(json["prior"]["source"], "parent");
        assert_eq!(json["prior"]["tasks"], 12);
    }
}
