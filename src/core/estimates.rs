//! Estimates: `Core` reads the snapshot (`History`) the pure `estimate`
//! module works on. The TUI keeps the `History` (`history`); the CLI asks
//! once (`estimate`).

use crate::{
    Res,
    core::Core,
    estimate::{self, Estimate, History},
    model::{
        sessions::{SessionQuery, SessionStore},
        time::Time,
    },
};

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
}

#[cfg(test)]
mod tests {
    use crate::{
        estimate::Basis,
        model::time::Minutes,
        test_util::{at, core, task}, // core: disk_tree, root: [a, ws: [b]]
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
}
