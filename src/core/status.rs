//! Task status: only "done" is stored; "started" / "to do" come from the
//! sessions (`Task::status`), so the views ask `tasks_with_sessions`.

use std::collections::HashSet;

use crate::{
    Res,
    core::Core,
    model::{
        id::NodeId,
        sessions::{Session, SessionQuery, SessionStore},
        time::Time,
    },
};

impl Core {
    /// Mark the task at `path` done at `now` (`done`) or reopen it, and save
    /// it. Not a task: error. Marking the task the timer runs on done stops
    /// the timer first (at `now`); that session is returned. Callers pass
    /// `time::now()`.
    pub async fn set_done(
        &mut self,
        path: &[usize],
        done: bool,
        now: Time,
    ) -> Res<Option<Session>> {
        self.changed();
        let Some(id) = self
            .tree
            .get(path)
            .and_then(|n| n.as_task().map(|_| n.id()))
        else {
            return Err("only tasks can be done".into()); // checked before stopping anything
        };
        let stopped = match done {
            true => self.stop_if_on(&[id], now).await?,
            false => None,
        };
        self.tree.set_task_done(path, done.then_some(now)).await?;
        Ok(stopped)
    }

    /// Mark the task at `path` done at `at`, back-dated (`udo done --at`,
    /// #67). Refused: `at` after `now`; before a session on the task ended
    /// (it would have time after it was done); without `force`, a task
    /// already done (`force`: its done time changes) and one whose timer
    /// runs (`force`: the timer stops at `at`, and that session is
    /// returned). Never before the timer's start: the store would remove
    /// the session (its rule for a clock error). Callers pass
    /// `time::now()`.
    pub async fn done_at(
        &mut self,
        path: &[usize],
        at: Time,
        now: Time,
        force: bool,
    ) -> Res<Option<Session>> {
        self.changed();
        let Some((name, done)) = self
            .tree
            .get(path)
            .and_then(|n| Some((n.name().to_string(), n.as_task()?.done_at.is_some())))
        else {
            return Err("only tasks can be done".into());
        };
        if done && !force {
            return Err(format!("{name} is already done: --force to change its done time").into());
        }
        if at > now {
            return Err("that time is in the future".into());
        }
        let sessions = self.sessions_of(path).await?;
        if sessions.iter().filter_map(|s| s.end).any(|end| end > at) {
            return Err(format!(
                "{name} has a session ending after that time: done can't be earlier"
            )
            .into());
        }
        let stopped = match sessions.iter().find(|s| s.end.is_none()) {
            None => None,
            Some(_) if !force => {
                return Err(format!(
                    "the timer runs on {name}: stop it first, or --force to stop it at that time"
                )
                .into());
            }
            Some(running) if at <= running.start => {
                return Err(format!("the timer on {name} started after that time").into());
            }
            Some(_) => self.stop(at).await?,
        };
        self.tree.set_task_done(path, Some(at)).await?;
        Ok(stopped)
    }

    /// Ids of the tasks with at least one (not removed) session, running
    /// ones included.
    pub async fn tasks_with_sessions(&self) -> Res<HashSet<NodeId>> {
        let sessions = self
            .storage
            .sessions
            .query(&SessionQuery::default())
            .await?;
        Ok(sessions.into_iter().map(|s| s.task.id).collect())
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        model::{node::Node, sessions::SessionStore, tree::Tree},
        test_util::{at, core}, // core: disk_tree, root: [a, ws: [b]]
    };

    #[tokio::test]
    async fn done_is_set_and_saved() {
        let (tmp, mut core) = core().await;

        core.set_done(&[0], true, at(14, 0)).await.unwrap();

        let reloaded = Tree::load_from(tmp.path()).await.unwrap();
        let a = reloaded.get(&[0]).and_then(Node::as_task).unwrap();
        assert_eq!(a.done_at, Some(at(14, 0)));
    }

    #[tokio::test]
    async fn undo_clears_done() {
        let (_tmp, mut core) = core().await;
        core.set_done(&[0], true, at(14, 0)).await.unwrap();

        core.set_done(&[0], false, at(15, 0)).await.unwrap();

        let a = core.tree().get(&[0]).and_then(Node::as_task).unwrap();
        assert_eq!(a.done_at, None);
    }

    #[tokio::test]
    async fn containers_cannot_be_done() {
        let (_tmp, mut core) = core().await;

        let result = core.set_done(&[1], true, at(14, 0)).await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn done_on_the_timed_task_stops_the_timer() {
        let (_tmp, mut core) = core().await;
        core.start(&[0], at(14, 0)).await.unwrap();

        let stopped = core.set_done(&[0], true, at(14, 45)).await.unwrap();

        assert_eq!(stopped.unwrap().end, Some(at(14, 45)));
        assert_eq!(core.sessions().running().await.unwrap(), None);
    }

    #[tokio::test]
    async fn done_on_another_task_keeps_the_timer() {
        let (_tmp, mut core) = core().await;
        core.start(&[0], at(14, 0)).await.unwrap();

        let stopped = core.set_done(&[1, 0], true, at(14, 45)).await.unwrap();

        assert_eq!(stopped, None);
        assert!(core.sessions().running().await.unwrap().is_some());
    }

    // ---------- done_at: `udo done --at` (#67) ----------

    fn done_at_of(core: &crate::core::Core, path: &[usize]) -> Option<crate::model::time::Time> {
        core.tree()
            .get(path)
            .and_then(Node::as_task)
            .unwrap()
            .done_at
    }

    #[tokio::test]
    async fn done_at_sets_a_back_dated_done_and_saves_it() {
        let (tmp, mut core) = core().await;
        core.add_session(&[0], at(9, 0), at(10, 0), at(14, 0))
            .await
            .unwrap();

        let stopped = core.done_at(&[0], at(11, 0), at(14, 0), false).await;

        assert_eq!(stopped.unwrap(), None);
        let reloaded = Tree::load_from(tmp.path()).await.unwrap();
        let a = reloaded.get(&[0]).and_then(Node::as_task).unwrap();
        assert_eq!(a.done_at, Some(at(11, 0)));
    }

    #[tokio::test]
    async fn done_at_refuses_the_future() {
        let (_tmp, mut core) = core().await;

        let result = core.done_at(&[0], at(15, 0), at(14, 0), false).await;

        assert!(result.is_err());
        assert_eq!(done_at_of(&core, &[0]), None);
    }

    /// Done before the work on it ended: the task would have time after it
    /// was done.
    #[tokio::test]
    async fn done_at_refuses_a_time_before_the_work_ended() {
        let (_tmp, mut core) = core().await;
        core.add_session(&[0], at(9, 0), at(10, 0), at(14, 0))
            .await
            .unwrap();

        let result = core.done_at(&[0], at(9, 30), at(14, 0), false).await;

        let e = result.unwrap_err().to_string();
        assert_eq!(
            e,
            "a has a session ending after that time: done can't be earlier"
        );
        assert_eq!(done_at_of(&core, &[0]), None);
    }

    #[tokio::test]
    async fn done_at_refuses_while_its_timer_runs() {
        let (_tmp, mut core) = core().await;
        core.start(&[0], at(12, 0)).await.unwrap();

        let result = core.done_at(&[0], at(13, 0), at(14, 0), false).await;

        let e = result.unwrap_err().to_string();
        assert_eq!(
            e,
            "the timer runs on a: stop it first, or --force to stop it at that time"
        );
        assert!(core.sessions().running().await.unwrap().is_some());
        assert_eq!(done_at_of(&core, &[0]), None);
    }

    #[tokio::test]
    async fn done_at_with_force_stops_its_timer_at_that_time() {
        let (_tmp, mut core) = core().await;
        core.start(&[0], at(12, 0)).await.unwrap();

        let stopped = core.done_at(&[0], at(13, 0), at(14, 0), true).await;

        assert_eq!(stopped.unwrap().unwrap().end, Some(at(13, 0)));
        assert_eq!(core.sessions().running().await.unwrap(), None);
        assert_eq!(done_at_of(&core, &[0]), Some(at(13, 0)));
    }

    /// Ending the session before its start would remove it (the store's
    /// rule for a clock error): refused, also with `--force`.
    #[tokio::test]
    async fn force_cannot_stop_the_timer_before_it_started() {
        let (_tmp, mut core) = core().await;
        let running = core.start(&[0], at(12, 0)).await.unwrap();

        let result = core.done_at(&[0], at(11, 0), at(14, 0), true).await;

        let e = result.unwrap_err().to_string();
        assert_eq!(e, "the timer on a started after that time");
        assert_eq!(core.sessions().running().await.unwrap(), Some(running));
        assert_eq!(done_at_of(&core, &[0]), None);
    }

    #[tokio::test]
    async fn done_at_keeps_another_tasks_timer() {
        let (_tmp, mut core) = core().await;
        core.start(&[1, 0], at(12, 0)).await.unwrap();

        let stopped = core.done_at(&[0], at(13, 0), at(14, 0), false).await;

        assert_eq!(stopped.unwrap(), None);
        assert!(core.sessions().running().await.unwrap().is_some());
        assert_eq!(done_at_of(&core, &[0]), Some(at(13, 0)));
    }

    #[tokio::test]
    async fn done_at_refuses_a_container() {
        let (_tmp, mut core) = core().await;

        assert!(
            core.done_at(&[1], at(13, 0), at(14, 0), false)
                .await
                .is_err()
        );
    }

    /// Already done: a new time only with `force` (it changes the done time).
    #[tokio::test]
    async fn done_at_on_a_done_task_needs_force() {
        let (_tmp, mut core) = core().await;
        core.set_done(&[0], true, at(12, 0)).await.unwrap();

        let refused = core.done_at(&[0], at(11, 0), at(14, 0), false).await;
        assert_eq!(
            refused.unwrap_err().to_string(),
            "a is already done: --force to change its done time"
        );
        assert_eq!(done_at_of(&core, &[0]), Some(at(12, 0)));

        core.done_at(&[0], at(11, 0), at(14, 0), true)
            .await
            .unwrap();
        assert_eq!(done_at_of(&core, &[0]), Some(at(11, 0)));
    }

    #[tokio::test]
    async fn tasks_with_sessions_follows_the_sessions() {
        let (_tmp, core) = core().await;
        let a = core.tree().get(&[0]).unwrap().id();
        let b = core.tree().get(&[1, 0]).unwrap().id();
        assert!(core.tasks_with_sessions().await.unwrap().is_empty());

        let s = core
            .add_session(&[0], at(9, 0), at(10, 0), at(20, 0))
            .await
            .unwrap();
        let with = core.tasks_with_sessions().await.unwrap();
        assert!(with.contains(&a) && !with.contains(&b));

        core.delete_session(s.id).await.unwrap(); // its last session
        assert!(!core.tasks_with_sessions().await.unwrap().contains(&a));
    }

    #[tokio::test]
    async fn a_running_session_counts() {
        let (_tmp, mut core) = core().await;
        let a = core.tree().get(&[0]).unwrap().id();

        core.start(&[0], at(14, 0)).await.unwrap();

        assert!(core.tasks_with_sessions().await.unwrap().contains(&a));
    }
}
