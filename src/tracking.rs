//! Timing tasks by hand: what the CLI and the TUI do on start / stop, and
//! the lines both show.

use crate::{
    Res,
    model::{
        sessions::{Session, SessionSource, SessionStore, TaskRef},
        task::TaskStatus,
        time::Time,
        tree::Tree,
    },
    storage::Storage,
};

/// Start timing the task at `path` at `at` (a running one is stopped first)
/// and mark it in progress. Callers pass `time::now()`.
pub async fn start(tree: &mut Tree, storage: &Storage, path: &[usize], at: Time) -> Res<Session> {
    let (_, parent_path) = path.split_last().ok_or("not a task")?; // the root row has no parent
    let task = TaskRef::of(
        tree.get(path).ok_or("no such node")?,
        tree.get(parent_path).ok_or("no such node")?,
    )
    .ok_or("not a task")?;

    let session = storage
        .sessions
        .start(task, SessionSource::Manual, at)
        .await?;
    tree.set_task_status(path, TaskStatus::InProgress).await?;
    Ok(session)
}

/// Stop the running session at `at`, if any. Callers pass `time::now()`.
pub async fn stop(storage: &Storage, at: Time) -> Res<Option<Session>> {
    Ok(storage.sessions.stop(at).await?)
}

/// `▶ lab 3 · 1h12`: the running session, timed up to `now`.
pub fn running_line(session: &Session, now: Time) -> String {
    format!("▶ {} · {}", session.task.name, session.duration(now))
}

/// `■ lab 3 · 45m`: a stopped session. A running one counts as 0 minutes.
pub fn stopped_line(session: &Session) -> String {
    let duration = session.duration(session.start); // `now` only counts while running
    format!("■ {} · {duration}", session.task.name)
}

#[cfg(test)]
mod tests {
    use chrono::{FixedOffset, TimeZone};
    use tempfile::TempDir;

    use super::*;
    use crate::{
        model::{container::ContainerKind, node::Node, sessions::SessionQuery},
        test_util::{container_at, task},
    };

    /// 2026-10-15 at `h:m`, offset +02:00.
    fn at(h: u32, m: u32) -> Time {
        FixedOffset::east_opt(2 * 3600)
            .unwrap()
            .with_ymd_and_hms(2026, 10, 15, h, m, 0)
            .unwrap()
    }

    /// root (tmp) -> [task "a", ws (tmp/ws) -> [task "b"]], saved: starting
    /// writes the status into `.udo.toml`.
    async fn disk_tree() -> (TempDir, Tree) {
        let tmp = tempfile::tempdir().unwrap();
        let ws_dir = tmp.path().join("ws");
        std::fs::create_dir(&ws_dir).unwrap();
        let tree = Tree::new(container_at(
            "root",
            tmp.path(),
            ContainerKind::Root,
            vec![
                task("a"),
                container_at("ws", &ws_dir, ContainerKind::Workspace, vec![task("b")]),
            ],
        ));
        tree.save(&[]).await.unwrap();
        tree.save(&[1]).await.unwrap();
        (tmp, tree)
    }

    fn status(tree: &Tree, path: &[usize]) -> TaskStatus {
        tree.get(path).and_then(Node::as_task).unwrap().status
    }

    async fn all(storage: &Storage) -> Vec<Session> {
        storage
            .sessions
            .query(&SessionQuery::default())
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn start_times_the_task_and_marks_it_in_progress() {
        let (tmp, mut tree) = disk_tree().await;
        let storage = Storage::in_memory();

        let session = start(&mut tree, &storage, &[1, 0], at(14, 0))
            .await
            .unwrap();

        assert_eq!(session.task.name, "b");
        assert_eq!(session.task.container_id, tree.get(&[1]).unwrap().id());
        assert_eq!(session.task.container_dir, tmp.path().join("ws"));
        assert_eq!(session.start, at(14, 0));
        assert_eq!(storage.sessions.running().await.unwrap(), Some(session));
        assert_eq!(status(&tree, &[1, 0]), TaskStatus::InProgress);
    }

    #[tokio::test]
    async fn the_status_is_saved() {
        let (tmp, mut tree) = disk_tree().await;

        start(&mut tree, &Storage::in_memory(), &[0], at(14, 0))
            .await
            .unwrap();

        let reloaded = Tree::load_from(tmp.path()).await.unwrap();
        assert_eq!(status(&reloaded, &[0]), TaskStatus::InProgress);
    }

    /// A task directly in the root: the root is its container.
    #[tokio::test]
    async fn a_task_in_the_root_belongs_to_the_root() {
        let (tmp, mut tree) = disk_tree().await;

        let session = start(&mut tree, &Storage::in_memory(), &[0], at(14, 0))
            .await
            .unwrap();

        assert_eq!(session.task.container_id, tree.root.id());
        assert_eq!(session.task.container_dir, tmp.path());
    }

    #[tokio::test]
    async fn starting_another_task_stops_the_first() {
        let (_tmp, mut tree) = disk_tree().await;
        let storage = Storage::in_memory();
        let first = start(&mut tree, &storage, &[0], at(14, 0)).await.unwrap();

        let second = start(&mut tree, &storage, &[1, 0], at(15, 0))
            .await
            .unwrap();

        assert_eq!(storage.sessions.running().await.unwrap(), Some(second));
        let first = all(&storage).await.into_iter().find(|s| s.id == first.id);
        assert_eq!(first.unwrap().end, Some(at(15, 0)));
        // stopped, not finished: it stays in progress
        assert_eq!(status(&tree, &[0]), TaskStatus::InProgress);
    }

    #[tokio::test]
    async fn the_root_and_containers_cannot_be_started() {
        let (_tmp, mut tree) = disk_tree().await;
        let storage = Storage::in_memory();

        for path in [&[][..], &[1]] {
            assert!(start(&mut tree, &storage, path, at(14, 0)).await.is_err());
        }

        assert!(all(&storage).await.is_empty());
    }

    #[tokio::test]
    async fn a_missing_path_is_an_error() {
        let (_tmp, mut tree) = disk_tree().await;
        let storage = Storage::in_memory();

        assert!(start(&mut tree, &storage, &[7], at(14, 0)).await.is_err());
        assert!(all(&storage).await.is_empty());
    }

    /// The session comes first: a start the store refuses leaves the status
    /// alone.
    #[tokio::test]
    async fn a_refused_start_keeps_the_status() {
        let (_tmp, mut tree) = disk_tree().await;
        let storage = Storage::in_memory();
        start(&mut tree, &storage, &[0], at(14, 0)).await.unwrap();
        stop(&storage, at(15, 0)).await.unwrap();

        let inside = start(&mut tree, &storage, &[1, 0], at(14, 30)).await;

        assert!(inside.is_err()); // over the recorded 14:00-15:00
        assert_eq!(status(&tree, &[1, 0]), TaskStatus::Pending);
    }

    #[tokio::test]
    async fn stop_ends_the_running_session() {
        let (_tmp, mut tree) = disk_tree().await;
        let storage = Storage::in_memory();
        start(&mut tree, &storage, &[0], at(14, 0)).await.unwrap();

        let stopped = stop(&storage, at(15, 0)).await.unwrap().unwrap();

        assert_eq!(stopped.end, Some(at(15, 0)));
        assert_eq!(storage.sessions.running().await.unwrap(), None);
    }

    #[tokio::test]
    async fn stop_without_a_running_session_is_none() {
        assert_eq!(stop(&Storage::in_memory(), at(15, 0)).await.unwrap(), None);
    }

    // ---------- lines ----------

    #[tokio::test]
    async fn the_running_line_shows_name_and_time_so_far() {
        let (_tmp, mut tree) = disk_tree().await;
        let session = start(&mut tree, &Storage::in_memory(), &[0], at(14, 0))
            .await
            .unwrap();

        assert_eq!(running_line(&session, at(15, 12)), "▶ a · 1h12");
    }

    #[tokio::test]
    async fn the_stopped_line_shows_name_and_length() {
        let (_tmp, mut tree) = disk_tree().await;
        let storage = Storage::in_memory();
        start(&mut tree, &storage, &[0], at(14, 0)).await.unwrap();

        let stopped = stop(&storage, at(14, 45)).await.unwrap().unwrap();

        assert_eq!(stopped_line(&stopped), "■ a · 45m");
    }
}
