//! Estimates: `Core` reads the snapshot (`History`) the pure `estimate`
//! module works on. The TUI keeps the `History` (`history`); the CLI asks
//! once (`estimate`). And it records them into the history (#47) when a
//! task is created or gets its first session (`record_estimate`).

use crate::{
    Res,
    core::Core,
    estimate::{
        self, Average, Estimate, Estimator, History,
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

    /// The estimate for the node at `path` (`estimate::of_node`); `None`:
    /// no such node, or nothing to estimate from. Reads the whole history:
    /// for one answer (CLI), not per frame.
    pub async fn estimate(&self, path: &[usize], now: Time) -> Res<Option<Estimate>> {
        let history = self.history(now).await?;
        Ok(self
            .tree
            .get(path)
            .and_then(|n| estimate::of_node(n, &history)))
    }

    /// Write the estimate of the task at `path` into the history (#47), as
    /// udo gives it at the task's `frozen_at`: its creation (`Created`) or
    /// its first session (`Started`; call after the session is stored).
    /// "First" is #8's: the earliest session not removed, so a session
    /// added before the old first one is a new first. Never fails the
    /// action it follows: an error becomes a warning. A container, a later
    /// session, nothing to estimate from, or a repeat: no row.
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
        let reason = match moment {
            Moment::Created => Reason::Created,
            // only the task's first session is a moment (#8 freezes there)
            Moment::Started { start } if task.first_session == Some(start) => Reason::Started,
            Moment::Started { .. } => return Ok(()),
        };
        let estimator = Average; // the one `of_node` shows
        let Some(estimate) = estimator.estimate(id, &history.as_of(task.frozen_at())) else {
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
        estimate::Basis,
        estimate::store::{EstimateStore, NewEstimate, Reason},
        model::container::ContainerKind,
        model::{id::NodeId, sessions::SessionStore, settings::ContainerSettings, time::Minutes},
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

        let estimate = core.estimate(&c, at(12, 0)).await.unwrap().unwrap();

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
    async fn a_container_gets_its_average() {
        let (_tmp, mut core) = core().await;
        core.add_session(&[1, 0], at(9, 0), at(10, 0), at(12, 0))
            .await
            .unwrap();
        core.set_done(&[1, 0], true, at(11, 0)).await.unwrap();

        let estimate = core.estimate(&[1], at(12, 0)).await.unwrap().unwrap();

        assert_eq!(estimate.minutes, Minutes::new(60));
    }

    #[tokio::test]
    async fn nothing_to_estimate_from_is_none() {
        let (_tmp, core) = core().await;

        assert_eq!(core.estimate(&[0], at(12, 0)).await.unwrap(), None);
    }

    #[tokio::test]
    async fn unknown_path_is_none() {
        let (_tmp, core) = core().await;

        assert_eq!(core.estimate(&[7, 7], at(12, 0)).await.unwrap(), None);
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
}
