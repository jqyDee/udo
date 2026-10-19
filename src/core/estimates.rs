//! Estimates: `Core` reads the snapshot (`History`) the pure `estimate`
//! module works on. The TUI keeps the `History` and every task's `Shown`
//! (`estimates`); the CLI asks once (`estimate`, `totals`). And it records
//! them into the history (#47) when a task is created, gets its first
//! session, or its own estimate changes (#6; `record_estimate`).

use crate::{
    Res,
    core::Core,
    estimate::{
        self, Average, Estimator, History, Shown,
        store::{EstimateStore, NewEstimate, Reason},
    },
    model::{
        sessions::{SessionQuery, SessionStore},
        time::Time,
    },
};

/// When a history row may be due, with what the check needs.
pub(super) enum Moment {
    /// The task was just created.
    Created,
    /// A session starting at `start` (as stored) was just stored for the
    /// task: a row only if it is the task's first.
    Started { start: Time },
    /// The task's own estimate was set, changed or removed (`Core::edit`;
    /// only on a real change).
    Edited,
}

impl Core {
    /// The tree and every (not removed) session, read once; a running
    /// session counts up to `now`.
    pub async fn history(&self, now: Time) -> Res<History> {
        let sessions = self
            .storage
            .sessions
            .query(&SessionQuery::default())
            .await?;
        Ok(History::build(&self.tree, &sessions, now))
    }

    /// What is shown for the node at `path` (`estimate::of_node`): your own
    /// estimate and the learned one. Both `None`: no such node, nothing
    /// set, nothing to learn from. Reads the whole history: for one answer
    /// (CLI), not per frame.
    pub async fn estimate(&self, path: &[usize], now: Time) -> Res<Shown> {
        let history = self.history(now).await?;
        Ok(self
            .tree
            .get(path)
            .map(|n| estimate::of_node(n, &history))
            .unwrap_or_default())
    }

    /// The history and every task's `Shown` (`Estimates`), read and computed
    /// once: the TUI per `rebuild` (when the data changed), `ls --time` per
    /// call.
    pub async fn estimates(&self, now: Time) -> Res<(History, estimate::Estimates)> {
        let history = self.history(now).await?;
        let estimates = estimate::Estimates::build(&self.tree, &history);
        Ok((history, estimates))
    }

    /// Estimate vs. actual over the tasks at or below `path`
    /// (`estimate::totals`); a task: itself alone. No such node: all 0.
    /// Builds everything: for one answer (CLI), not per frame.
    pub async fn totals(&self, path: &[usize], now: Time) -> Res<estimate::Totals> {
        let (history, estimates) = self.estimates(now).await?;
        Ok(estimate::totals(&self.tree, path, &estimates, &history))
    }

    /// Write the estimate of the task at `path` into the history (#47), as
    /// udo goes by it (`estimate::pick`: yours, else the learned one) at
    /// the task's `frozen_at`: its creation (`Created`) or its first
    /// session (`Started`; call after the session is stored). "First" is
    /// #8's: the earliest session not removed, so a session added before
    /// the old first one is a new first. `Edited` (#6, after the edit):
    /// your own estimate as set now (`manual`), or, removed, the learned
    /// one as shown from now on (`cleared`). Never fails the action it
    /// follows: an error becomes a warning. A container, a later session,
    /// nothing to estimate from, or a repeat: no row.
    pub(super) async fn record_estimate(&self, path: &[usize], moment: Moment, now: Time) {
        if let Err(e) = self.try_record_estimate(path, moment, now).await {
            self.warn(format!("estimate not recorded: {e}"));
        }
    }

    async fn try_record_estimate(&self, path: &[usize], moment: Moment, now: Time) -> Res<()> {
        let Some(node) = self.tree.get(path).filter(|n| n.as_task().is_some()) else {
            return Ok(()); // a container (or gone): no row
        };
        let id = node.id();
        let history = self.history(now).await?;
        let Some(task) = history.task(id) else {
            return Ok(());
        };
        // as at `frozen_at`; `as_of` keeps the own estimate, so `pick` gives yours
        let then = || history.as_of(task.frozen_at());
        let (reason, picked) = match moment {
            Moment::Created => (Reason::Created, estimate::pick(id, &then())),
            // only the task's first session is a moment (#8 freezes there)
            Moment::Started { start } if task.first_session == Some(start) => {
                (Reason::Started, estimate::pick(id, &then()))
            }
            Moment::Started { .. } => return Ok(()),
            // set or changed: `pick` gives yours (`Manual`), never frozen
            Moment::Edited if task.own_estimate.is_some() => {
                (Reason::Manual, estimate::pick(id, &history))
            }
            // removed: what udo goes by from now on, the learned estimate as
            // shown (frozen for a done task)
            Moment::Edited => {
                let learned = estimate::of_node(node, &history).learned;
                let average: &'static dyn Estimator = &Average; // the one `of_node` learns with
                (Reason::Cleared, learned.map(|e| (e, average)))
            }
        };
        let Some((estimate, estimator)) = picked else {
            return Ok(()); // nothing set, no data: no row
        };
        let row = NewEstimate::of(
            id,
            &estimate,
            estimator.method(),
            estimator.version(),
            reason,
        );
        self.storage.estimates.record(row).await?; // `None` (a repeat) is fine
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::Moment;
    use crate::{
        core::Core,
        estimate::{
            Basis, Shown,
            store::{EstimateStore, NewEstimate, Reason},
        },
        model::{
            container::ContainerKind,
            id::NodeId,
            node::{BodyPatch, HeaderPatch, NodeBody, NodePatch},
            sessions::SessionStore,
            settings::ContainerSettings,
            task::TaskPatch,
            time::Minutes,
        },
        // core: disk_tree, root: [a, ws: [b]]
        test_util::{at, core, core_with_broken_estimates, new_container, task},
    };

    #[tokio::test]
    async fn history_has_the_recorded_sessions() {
        let (_tmp, core) = core().await;
        core.add_session(&[1, 0], at(9, 0), at(10, 0), at(12, 0))
            .await
            .unwrap();

        let history = core.history(at(12, 0)).await.unwrap();

        let b = core.tree().get(&[1, 0]).unwrap().id();
        assert_eq!(history.task(b).unwrap().actual, Minutes::new(60));
    }

    #[tokio::test]
    async fn history_counts_a_running_session_up_to_now() {
        let (_tmp, mut core) = core().await;
        core.start(&[1, 0], at(9, 0)).await.unwrap();

        let history = core.history(at(9, 30)).await.unwrap();

        let b = core.tree().get(&[1, 0]).unwrap().id();
        assert_eq!(history.task(b).unwrap().actual, Minutes::new(30));
    }

    #[tokio::test]
    async fn a_task_gets_what_its_container_learned() {
        // ws: [b (done, 1h), c]: c's estimate is learned from b
        let (_tmp, mut core) = core().await;
        core.add_session(&[1, 0], at(9, 0), at(10, 0), at(12, 0))
            .await
            .unwrap();
        core.set_done(&[1, 0], true, at(11, 0)).await.unwrap();
        let c = core.create(&[1], task("c")).await.unwrap();

        let estimate = core.estimate(&c, at(12, 0)).await.unwrap().learned.unwrap();

        assert_eq!(estimate.minutes, Minutes::new(60));
        assert!(matches!(
            estimate.basis,
            Basis::Learned {
                done_tasks: 1,
                open_tasks: 0,
                ..
            }
        ));
    }

    #[tokio::test]
    async fn estimates_give_every_task_its_shown() {
        // ws: [b (done, 1h), c]: c's estimate is learned from b
        let (_tmp, mut core) = core().await;
        core.add_session(&[1, 0], at(9, 0), at(10, 0), at(12, 0))
            .await
            .unwrap();
        core.set_done(&[1, 0], true, at(11, 0)).await.unwrap();
        let c_path = core.create(&[1], task("c")).await.unwrap();

        let (history, estimates) = core.estimates(at(12, 0)).await.unwrap();

        let b = core.tree().get(&[1, 0]).unwrap().id();
        let c = core.tree().get(&c_path).unwrap().id();
        assert_eq!(history.task(b).unwrap().actual, Minutes::new(60));
        let one_answer = core.estimate(&c_path, at(12, 0)).await.unwrap();
        assert_eq!(estimates.get(c), one_answer);
        assert!(one_answer.learned.is_some()); // not two empty ones
    }

    #[tokio::test]
    async fn totals_sum_the_tasks_below() {
        // ws: [b (done, 1h), c (open, learns 1h from b, nothing tracked)]
        let (_tmp, mut core) = core().await;
        core.add_session(&[1, 0], at(9, 0), at(10, 0), at(12, 0))
            .await
            .unwrap();
        core.set_done(&[1, 0], true, at(11, 0)).await.unwrap();
        core.create(&[1], task("c")).await.unwrap();

        let ws = core.totals(&[1], at(12, 0)).await.unwrap();

        assert_eq!(ws.tasks, 2);
        assert_eq!(ws.actual, Minutes::new(60));
        assert_eq!(ws.left, Minutes::new(60)); // c: 1h estimated, 0 tracked
    }

    #[tokio::test]
    async fn totals_of_no_such_node_are_all_zero() {
        let (_tmp, core) = core().await;

        let none = core.totals(&[9], at(12, 0)).await.unwrap();

        assert_eq!(none, crate::estimate::Totals::default());
    }

    #[tokio::test]
    async fn a_container_gets_its_average() {
        let (_tmp, mut core) = core().await;
        core.add_session(&[1, 0], at(9, 0), at(10, 0), at(12, 0))
            .await
            .unwrap();
        core.set_done(&[1, 0], true, at(11, 0)).await.unwrap();

        let estimate = core
            .estimate(&[1], at(12, 0))
            .await
            .unwrap()
            .learned
            .unwrap();

        assert_eq!(estimate.minutes, Minutes::new(60));
    }

    #[tokio::test]
    async fn nothing_to_estimate_from_is_none() {
        let (_tmp, core) = core().await;

        assert_eq!(
            core.estimate(&[0], at(12, 0)).await.unwrap(),
            Shown::default()
        );
    }

    #[tokio::test]
    async fn unknown_path_is_none() {
        let (_tmp, core) = core().await;

        assert_eq!(
            core.estimate(&[7, 7], at(12, 0)).await.unwrap(),
            Shown::default()
        );
    }

    // ---------- record_estimate ----------

    /// The latest history row of the node at `path`.
    async fn last_row(core: &Core, path: &[usize]) -> Option<NewEstimate> {
        let id = core.tree().get(path).unwrap().id();
        let row = core.storage.estimates.last_of(id).await.unwrap();
        row.map(|r| r.estimate)
    }

    #[tokio::test]
    async fn a_setting_is_recorded_as_the_prior() {
        // ws (setting 1h30): [b]
        let (_tmp, mut core) = core().await;
        let settings = ContainerSettings {
            estimate: Some(Minutes::new(90)),
            ..Default::default()
        };
        core.set_settings(&[1], settings, None).await.unwrap();

        core.record_estimate(&[1, 0], Moment::Created, at(12, 0))
            .await;

        let row = last_row(&core, &[1, 0]).await.unwrap();
        assert_eq!(row.method, "prior");
        assert_eq!(row.minutes, Minutes::new(90));
        assert_eq!(row.prior, Some(Minutes::new(90)));
        assert_eq!(row.reason, Reason::Created);
        assert!(core.take_warnings().is_empty());
    }

    /// No row is no error: a task with nothing to estimate from, a
    /// container, a missing path.
    #[tokio::test]
    async fn nothing_to_estimate_from_records_nothing() {
        let (_tmp, core) = core().await;

        core.record_estimate(&[0], Moment::Created, at(12, 0)).await;
        core.record_estimate(&[1], Moment::Created, at(12, 0)).await; // a container
        core.record_estimate(&[7, 7], Moment::Created, at(12, 0))
            .await;

        let ids: Vec<NodeId> = [&[0][..], &[1]]
            .iter()
            .map(|p| core.tree().get(p).unwrap().id())
            .collect();
        assert!(
            core.storage
                .estimates
                .of_tasks(&ids)
                .await
                .unwrap()
                .is_empty()
        );
        assert!(core.take_warnings().is_empty());
    }

    /// The `started` row is the estimate at the first session, not today's:
    /// a task done after it does not count (what #8 freezes, too). Asked
    /// again late (as after a back-dated session): still the value then.
    #[tokio::test]
    async fn a_started_row_is_the_estimate_at_the_first_session() {
        // ws: [b (done, 1h by 11:00), c, d]; c starts at 12:00, d is done
        // with 3h afterwards. Created at 8:00: `task()` takes the real now,
        // which may lie after the sessions.
        let (_tmp, mut core) = core().await;
        core.add_session(&[1, 0], at(9, 0), at(10, 0), at(23, 0))
            .await
            .unwrap();
        core.set_done(&[1, 0], true, at(11, 0)).await.unwrap();
        let mut c = task("c");
        c.header.created_at = at(8, 0);
        let c = core.create(&[1], c).await.unwrap();
        let mut d = task("d");
        d.header.created_at = at(8, 0);
        let d = core.create(&[1], d).await.unwrap();
        core.add_session(&c, at(12, 0), at(12, 30), at(23, 0))
            .await
            .unwrap();
        core.add_session(&d, at(13, 0), at(16, 0), at(23, 0))
            .await
            .unwrap();
        core.set_done(&d, true, at(16, 0)).await.unwrap();

        let first = Moment::Started { start: at(12, 0) };
        core.record_estimate(&c, first, at(23, 0)).await;

        let row = last_row(&core, &c).await.unwrap();
        assert_eq!(row.minutes, Minutes::new(60)); // only b, not d's 3h
        assert_eq!(row.done_tasks, 1);
        assert_eq!(row.reason, Reason::Started);
    }

    // ---------- the history through the actions (#47) ----------

    /// Every history row of the node with `id`, oldest first.
    async fn rows(core: &Core, id: NodeId) -> Vec<NewEstimate> {
        let rows = core.storage.estimates.of_tasks(&[id]).await.unwrap();
        rows.into_iter().map(|r| r.estimate).collect()
    }

    /// ws gets the `estimate` setting `minutes`.
    async fn set_estimate(core: &mut Core, minutes: u32) {
        let settings = ContainerSettings {
            estimate: Some(Minutes::new(minutes)),
            ..Default::default()
        };
        core.set_settings(&[1], settings, None).await.unwrap();
    }

    /// Task `name` in ws, created at 8:00 (`task()` takes the real now,
    /// which may lie after the sessions); its path.
    async fn create_at_8(core: &mut Core, name: &str) -> Vec<usize> {
        let mut node = task(name);
        node.header.created_at = at(8, 0);
        core.create(&[1], node).await.unwrap()
    }

    /// b worked 9–10 and done at 11: what ws's other tasks learn from.
    async fn b_done_in_an_hour(core: &mut Core) {
        core.add_session(&[1, 0], at(9, 0), at(10, 0), at(11, 0))
            .await
            .unwrap();
        core.set_done(&[1, 0], true, at(11, 0)).await.unwrap();
    }

    fn id(core: &Core, path: &[usize]) -> NodeId {
        core.tree().get(path).unwrap().id()
    }

    #[tokio::test]
    async fn creating_a_task_records_its_estimate() {
        let (tmp, mut core) = core().await;
        set_estimate(&mut core, 90).await;

        let c = create_at_8(&mut core, "c").await;
        let project = new_container("p", &tmp.path().join("p"), ContainerKind::Project);
        let p = core.create(&[1], project).await.unwrap();

        let [row] = &rows(&core, id(&core, &c)).await[..] else {
            panic!("one row");
        };
        assert_eq!(
            (row.method.as_str(), row.minutes),
            ("prior", Minutes::new(90))
        );
        assert_eq!(row.reason, Reason::Created);
        assert!(rows(&core, id(&core, &p)).await.is_empty()); // a container: none
        assert!(core.take_warnings().is_empty());
    }

    /// Only the first session is a moment. A later one gets no row, even
    /// when the estimate at the first session would now come out different:
    /// settings have no history, so it would be a recomputation, not what
    /// udo showed then.
    #[tokio::test]
    async fn only_the_first_start_records() {
        let (_tmp, mut core) = core().await;
        set_estimate(&mut core, 90).await;
        let c = create_at_8(&mut core, "c").await;
        b_done_in_an_hour(&mut core).await;

        core.start(&c, at(12, 0)).await.unwrap(); // learned from b: a change
        core.stop(at(12, 30)).await.unwrap();
        set_estimate(&mut core, 300).await;
        core.start(&c, at(17, 0)).await.unwrap();

        let reasons: Vec<Reason> = rows(&core, id(&core, &c))
            .await
            .iter()
            .map(|r| r.reason)
            .collect();
        assert_eq!(reasons, [Reason::Created, Reason::Started]);
    }

    #[tokio::test]
    async fn only_the_first_added_session_records() {
        let (_tmp, mut core) = core().await;
        set_estimate(&mut core, 90).await;
        let c = create_at_8(&mut core, "c").await;
        b_done_in_an_hour(&mut core).await;

        core.add_session(&c, at(12, 0), at(12, 30), at(23, 0))
            .await
            .unwrap();
        set_estimate(&mut core, 300).await;
        core.add_session(&c, at(13, 0), at(13, 30), at(23, 0))
            .await
            .unwrap();

        let rows = rows(&core, id(&core, &c)).await;
        let reasons: Vec<Reason> = rows.iter().map(|r| r.reason).collect();
        assert_eq!(reasons, [Reason::Created, Reason::Started]);
        assert_ne!(rows[1].minutes, Minutes::new(90)); // learned from b
    }

    /// Append-only: deleting the task keeps its rows (training data).
    #[tokio::test]
    async fn rows_stay_when_the_task_is_deleted() {
        let (_tmp, mut core) = core().await;
        set_estimate(&mut core, 90).await;
        let c = create_at_8(&mut core, "c").await;
        let c_id = id(&core, &c); // the path is gone afterwards

        core.delete(&c, at(13, 0)).await.unwrap();

        assert_eq!(rows(&core, c_id).await.len(), 1);
    }

    /// A failing store never undoes the action: each one stands, and each
    /// failed row becomes one warning.
    #[tokio::test]
    async fn a_failing_store_warns_and_the_action_stands() {
        let (_tmp, mut core) = core_with_broken_estimates().await;
        set_estimate(&mut core, 90).await; // so there is a row to write

        let c = create_at_8(&mut core, "c").await;
        let created = core.take_warnings();
        let started = core.start(&c, at(12, 0)).await.unwrap();
        let on_start = core.take_warnings();
        let added = core
            .add_session(&[1, 0], at(9, 0), at(10, 0), at(23, 0))
            .await
            .unwrap();
        let on_add = core.take_warnings();

        assert_eq!(core.tree().get(&c).unwrap().name(), "c");
        let running = core.sessions().running().await.unwrap();
        assert_eq!(running, Some(started));
        assert_eq!(core.sessions_of(&[1, 0]).await.unwrap(), vec![added]);
        for warnings in [created, on_start, on_add] {
            let [warning] = &warnings[..] else {
                panic!("one warning per action: {warnings:?}");
            };
            assert!(warning.starts_with("estimate not recorded: "), "{warning}");
        }
    }

    // ---------- a task's own estimate (#6) ----------

    /// The task at `path` gets its own estimate (`Some`) or loses it
    /// (`None`), through `Core::edit`.
    async fn set_own(core: &mut Core, path: &[usize], minutes: Option<u32>) {
        let patch = NodePatch {
            body: Some(BodyPatch::Task(TaskPatch {
                estimate: Some(minutes.map(Minutes::new)),
                ..Default::default()
            })),
            ..Default::default()
        };
        core.edit(path, patch).await.unwrap();
    }

    /// `(method, minutes, reason)` of every row of the node at `path`.
    async fn summary(core: &Core, path: &[usize]) -> Vec<(String, u32, Reason)> {
        rows(core, id(core, path))
            .await
            .into_iter()
            .map(|r| (r.method, r.minutes.get(), r.reason))
            .collect()
    }

    #[tokio::test]
    async fn own_estimate_set_changed_and_removed_are_recorded() {
        let (_tmp, mut core) = core().await;
        set_estimate(&mut core, 90).await; // so a removal has a learned value
        let c = create_at_8(&mut core, "c").await;

        set_own(&mut core, &c, Some(45)).await;
        set_own(&mut core, &c, Some(45)).await; // the same: no change, no row
        set_own(&mut core, &c, Some(60)).await;
        set_own(&mut core, &c, None).await;

        assert_eq!(
            summary(&core, &c).await,
            [
                ("prior".into(), 90, Reason::Created),
                ("manual".into(), 45, Reason::Manual),
                ("manual".into(), 60, Reason::Manual),
                ("prior".into(), 90, Reason::Cleared),
            ]
        );
        assert!(core.take_warnings().is_empty());
    }

    #[tokio::test]
    async fn a_task_created_with_its_own_estimate_records_it() {
        let (_tmp, mut core) = core().await;
        set_estimate(&mut core, 90).await;
        let mut node = task("c");
        node.header.created_at = at(8, 0);
        let NodeBody::Task(t) = &mut node.body else {
            unreachable!("task() builds a task");
        };
        t.estimate = Some(Minutes::new(45));
        let c = core.create(&[1], node).await.unwrap();

        core.start(&c, at(12, 0)).await.unwrap(); // the same 45m: a repeat

        assert_eq!(
            summary(&core, &c).await,
            [("manual".into(), 45, Reason::Created)]
        );
    }

    #[tokio::test]
    async fn removing_it_with_nothing_learned_records_nothing() {
        let (_tmp, mut core) = core().await; // no setting, no data
        let c = create_at_8(&mut core, "c").await;

        set_own(&mut core, &c, Some(45)).await;
        set_own(&mut core, &c, None).await;

        assert_eq!(
            summary(&core, &c).await,
            [("manual".into(), 45, Reason::Manual)]
        );
        assert!(core.take_warnings().is_empty());
    }

    /// Renames, due dates, and an edit form sending the unchanged (unset)
    /// estimate: no row.
    #[tokio::test]
    async fn other_edits_record_nothing() {
        let (_tmp, mut core) = core().await;
        set_estimate(&mut core, 90).await;
        let c = create_at_8(&mut core, "c").await;
        let patch = NodePatch {
            header: HeaderPatch {
                name: Some("c2".into()),
                ..Default::default()
            },
            body: Some(BodyPatch::Task(TaskPatch {
                due_date: Some(at(20, 0)),
                estimate: Some(None), // was not set either
                ..Default::default()
            })),
        };

        core.edit(&c, patch).await.unwrap();

        assert_eq!(
            summary(&core, &c).await,
            [("prior".into(), 90, Reason::Created)]
        );
    }

    /// Removed on a done task: the learned estimate as shown, frozen at its
    /// first session (d, done later with 5h, does not count).
    #[tokio::test]
    async fn removing_it_on_a_done_task_records_the_frozen_estimate() {
        let (_tmp, mut core) = core().await;
        set_estimate(&mut core, 90).await;
        b_done_in_an_hour(&mut core).await;
        let c = create_at_8(&mut core, "c").await;
        let d = create_at_8(&mut core, "d").await;
        core.add_session(&c, at(12, 0), at(12, 30), at(23, 0))
            .await
            .unwrap();
        core.set_done(&c, true, at(13, 0)).await.unwrap();
        core.add_session(&d, at(13, 0), at(18, 0), at(23, 0))
            .await
            .unwrap();
        core.set_done(&d, true, at(18, 0)).await.unwrap();

        set_own(&mut core, &c, Some(45)).await;
        set_own(&mut core, &c, None).await;

        let shown = core.estimate(&c, at(23, 0)).await.unwrap().learned.unwrap();
        assert_eq!(shown.as_of, Some(at(12, 0)));
        let last = rows(&core, id(&core, &c)).await.pop().unwrap();
        assert_eq!(last.reason, Reason::Cleared);
        assert_eq!(last.minutes, shown.minutes);
        assert_eq!(last.done_tasks, 1); // only b
    }

    #[tokio::test]
    async fn a_failing_store_keeps_the_own_estimate_and_warns() {
        let (_tmp, mut core) = core_with_broken_estimates().await;
        let c = create_at_8(&mut core, "c").await;
        core.take_warnings(); // nothing to record at creation anyway

        set_own(&mut core, &c, Some(45)).await;

        let task = core.tree().get(&c).and_then(|n| n.as_task()).unwrap();
        assert_eq!(task.estimate, Some(Minutes::new(45)));
        let warnings = core.take_warnings();
        let [warning] = &warnings[..] else {
            panic!("one warning: {warnings:?}");
        };
        assert!(warning.starts_with("estimate not recorded: "), "{warning}");
    }
}
