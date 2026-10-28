//! `udo estimate [NODE]`: what udo estimates for a task (or a typical task
//! in a container), and where that comes from: how many tasks it learned
//! from, and the prior it started from. A task with its own estimate shows
//! that first, udo's learned one below it (#6).

use std::{fmt, path::Path};

use chrono::Local;
use serde::Serialize;

use crate::{
    DATE_FMT, Res,
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

/// One node's estimate: your own (tasks only) and udo's learned one.
/// `minutes`, `done`, `open`, `prior` and `as_of` are always the learned
/// one, also when `own` is set (scripts keep working).
#[derive(Serialize)]
pub struct Estimated {
    pub id: NodeId,
    pub path: String,
    /// A container: "a typical task in it"; a task: its container's
    /// estimate, without its own time.
    pub container: bool,
    /// The task's own estimate (`udo edit --estimate`); wins over
    /// `minutes`. `None`: not set, or a container.
    pub own: Option<u32>,
    /// The learned estimate; `None`: nothing to learn from.
    pub minutes: Option<u32>,
    /// Done tasks learned from, at full weight.
    pub done: usize,
    /// Open tasks learned from: over the estimate from the done ones, at
    /// half weight. `done` and `open` both 0: only the prior.
    pub open: usize,
    pub prior: Option<PriorOut>,
    /// A done task: the moment its estimate is frozen at (its first
    /// session, else its creation); `None`: from today's data.
    pub as_of: Option<Time>,
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
    let shown = core.estimate(&path, now).await?;
    let estimate = shown.learned;

    let (done, open, prior) = match estimate.map(|e| e.basis) {
        Some(Basis::Learned {
            done_tasks,
            open_tasks,
            prior,
            ..
        }) => (done_tasks, open_tasks, prior),
        Some(Basis::Prior(p)) => (0, 0, Some(p)),
        Some(Basis::Own) | None => (0, 0, None), // `learned` is never `Own`
    };
    Ok(Estimated {
        id: node.id(),
        path: path_text(tree, &path),
        container: node.as_container().is_some(),
        own: shown.own.map(Minutes::get),
        minutes: estimate.map(|e| e.minutes.get()),
        done,
        open,
        prior: prior.map(|p| prior_out(tree, p)),
        as_of: estimate.and_then(|e| e.as_of),
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

/// `1 done task`, `2 open tasks`.
fn count(n: usize, what: &str) -> String {
    if n == 1 {
        format!("1 {what} task")
    } else {
        format!("{n} {what} tasks")
    }
}

/// Three lines: the estimate, what it learned from, the prior; a done task
/// gets a line after the first saying when it was estimated. Nothing to
/// estimate from: one line that says how to set a starting value. Your own
/// estimate: it first, `udo: <learned>` next, then the same lines (none
/// when there is nothing to learn from).
impl fmt::Display for Estimated {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(own) = self.own {
            write!(f, "{} for {} (yours)", Minutes::new(own), self.path)?;
            let Some(minutes) = self.minutes else {
                return write!(f, "\n  udo: nothing to learn from yet");
            };
            write!(f, "\n  udo: {}", Minutes::new(minutes))?;
            return self.fmt_learned(f);
        }
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
        self.fmt_learned(f)
    }
}

impl Estimated {
    /// The lines after the head: when a done task was estimated, what it
    /// learned from, the prior. Each starts with a newline.
    fn fmt_learned(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(at) = self.as_of {
            let at = at.with_timezone(&Local).format(DATE_FMT);
            write!(f, "\n  done: as estimated on {at}")?;
        }
        const OPEN: &str = "(over the estimate, half weight)";
        let learned = match (self.done, self.open) {
            (0, 0) => "no tasks with tracked time here yet".to_string(),
            (d, 0) => format!("learned from {}", count(d, "done")),
            (0, o) => format!("learned from {} {OPEN}", count(o, "open")),
            (d, o) => format!("learned from {} and {o} open {OPEN}", count(d, "done")),
        };
        write!(f, "\n  {learned}")?;
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
        model::{
            node::{BodyPatch, NodePatch},
            settings::ContainerSettings,
            task::TaskPatch,
        },
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

    /// b's own estimate.
    async fn set_b_estimate(core: &mut Core, minutes: u32) {
        let patch = NodePatch {
            body: Some(BodyPatch::Task(TaskPatch {
                estimate: Some(Some(Minutes::new(minutes))),
                ..Default::default()
            })),
            ..Default::default()
        };
        core.edit(&[1, 0], patch).await.unwrap();
    }

    /// An `Estimated` for the text tests: 1h20 from `done` and `open`
    /// tasks, `prior`.
    fn estimated(done: usize, open: usize, prior: Option<PriorOut>) -> Estimated {
        Estimated {
            id: NodeId::new(),
            path: "uni/cs".into(),
            container: true,
            own: None,
            minutes: Some(80),
            done,
            open,
            prior,
            as_of: None,
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
        assert_eq!((shown.minutes, shown.done, shown.open), (Some(60), 1, 0));
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
        assert_eq!((shown.minutes, shown.done, shown.open), (Some(90), 0, 0));
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
            "1h25 for a task in ws\n  learned from 1 done task\n  prior: 1h30, set on ws"
        );
    }

    #[tokio::test]
    async fn a_done_task_says_when_it_was_estimated() {
        // only that it says so: these nodes carry the real `now` as their
        // creation, so the exact moment depends on the day the test runs
        let (tmp, mut core) = core().await;
        set_ws_estimate(&mut core, 90).await;
        b_done_with(&mut core, 70).await;

        let shown = run(&core, tmp.path(), at(12, 0), &named("b"))
            .await
            .unwrap();

        assert!(shown.as_of.is_some());
        let text = render(&shown, false).unwrap();
        let second = text.lines().nth(1).unwrap();
        assert!(second.starts_with("  done: as estimated on "), "{text}");
        let json = serde_json::to_value(&shown).unwrap();
        assert!(json["as_of"].is_string());
    }

    #[tokio::test]
    async fn an_open_task_has_no_moment() {
        let (tmp, mut core) = core().await;
        set_ws_estimate(&mut core, 90).await;

        let shown = run(&core, tmp.path(), at(12, 0), &named("b"))
            .await
            .unwrap();

        assert_eq!(shown.as_of, None);
        assert!(!render(&shown, false).unwrap().contains("done:"));
    }

    #[test]
    fn text_puts_the_moment_after_the_estimate() {
        let mut shown = estimated(1, 0, None);
        shown.container = false;
        shown.path = "uni/cs/lab".into();
        shown.as_of = Some(crate::test_util::local(14, 0));

        let text = shown.to_string();

        assert_eq!(
            text,
            "1h20 for uni/cs/lab\n  done: as estimated on 2026-10-15 14:00\n  \
             learned from 1 done task\n  no prior: nothing set above"
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
        assert_eq!(json["done"], 0);
        assert_eq!(json["open"], 0);
        assert_eq!(json["prior"]["source"], "setting");
        assert_eq!(json["prior"]["from"], "ws");
        assert_eq!(json["prior"]["minutes"], 90);
        assert!(json["own"].is_null());
    }

    // ---------- a task's own estimate (#6) ----------

    #[tokio::test]
    async fn own_estimate_first_then_udos() {
        let (tmp, mut core) = core().await;
        set_ws_estimate(&mut core, 90).await;
        set_b_estimate(&mut core, 45).await;

        let shown = run(&core, tmp.path(), at(12, 0), &named("b"))
            .await
            .unwrap();

        assert_eq!((shown.own, shown.minutes), (Some(45), Some(90)));
        assert_eq!(
            render(&shown, false).unwrap(),
            "45m for ws/b (yours)\n  udo: 1h30\n  no tasks with tracked time here yet\n  \
             prior: 1h30, set on ws"
        );
    }

    #[tokio::test]
    async fn own_estimate_with_nothing_learned() {
        let (tmp, mut core) = core().await;
        set_b_estimate(&mut core, 45).await;

        let shown = run(&core, tmp.path(), at(12, 0), &named("b"))
            .await
            .unwrap();

        assert_eq!(
            render(&shown, false).unwrap(),
            "45m for ws/b (yours)\n  udo: nothing to learn from yet"
        );
    }

    #[tokio::test]
    async fn json_has_own_next_to_the_learned_fields() {
        let (tmp, mut core) = core().await;
        set_ws_estimate(&mut core, 90).await;
        set_b_estimate(&mut core, 45).await;

        let shown = run(&core, tmp.path(), at(12, 0), &named("b"))
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_str(&render(&shown, true).unwrap()).unwrap();

        assert_eq!(json["own"], 45);
        assert_eq!(json["minutes"], 90); // still the learned one
        assert_eq!(json["prior"]["source"], "setting");
    }

    #[test]
    fn text_puts_the_moment_after_udos_line() {
        let mut shown = estimated(1, 0, None);
        shown.container = false;
        shown.path = "uni/cs/lab".into();
        shown.own = Some(60);
        shown.as_of = Some(crate::test_util::local(14, 0));

        assert_eq!(
            shown.to_string(),
            "1h for uni/cs/lab (yours)\n  udo: 1h20\n  done: as estimated on 2026-10-15 14:00\n  \
             learned from 1 done task\n  no prior: nothing set above"
        );
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
    fn text_counts_several_done_tasks() {
        let text = estimated(6, 0, None).to_string();

        assert_eq!(
            text,
            "1h20 for a task in uni/cs\n  learned from 6 done tasks\n  no prior: nothing set above"
        );
    }

    #[test]
    fn text_explains_an_open_task() {
        let text = estimated(0, 1, None).to_string();

        assert!(text.contains("\n  learned from 1 open task (over the estimate, half weight)\n"));
    }

    #[test]
    fn text_explains_done_and_open_tasks() {
        let text = estimated(5, 1, None).to_string();

        assert!(text.contains(
            "\n  learned from 5 done tasks and 1 open (over the estimate, half weight)\n"
        ));
    }

    #[tokio::test]
    async fn an_open_task_over_the_estimate_is_shown_as_open() {
        // ws 1h; c open with 3h so far, b has nothing: b learns from c
        let (tmp, mut core) = core().await;
        set_ws_estimate(&mut core, 60).await;
        let c = core.create(&[1], task("c")).await.unwrap();
        core.add_session(&c, at(8, 0), at(11, 0), at(12, 0))
            .await
            .unwrap();

        let shown = run(&core, tmp.path(), at(12, 0), &named("b"))
            .await
            .unwrap();

        // (3·60 + ½·180) / 3.5 = 77.1
        assert_eq!((shown.minutes, shown.done, shown.open), (Some(77), 0, 1));
        let json = serde_json::to_value(&shown).unwrap();
        assert_eq!(
            (json["done"].clone(), json["open"].clone()),
            (0.into(), 1.into())
        );
    }

    #[test]
    fn text_names_a_pooled_prior() {
        let prior = PriorOut::Parent {
            from: "uni".into(),
            minutes: 105,
            tasks: 12,
        };

        let text = estimated(0, 0, Some(prior)).to_string();

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

        let json = serde_json::to_value(estimated(0, 0, Some(prior))).unwrap();

        assert_eq!(json["prior"]["source"], "parent");
        assert_eq!(json["prior"]["tasks"], 12);
    }
}
