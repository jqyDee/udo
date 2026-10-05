use crate::{
    core::Core,
    model::sessions::SessionStore,
    storage::{Storage, sqlite::DB_FILE_NAME},
    test_util::{at, core, disk_tree},
};

#[tokio::test]
async fn open_on_a_missing_dir_creates_the_root_and_the_database() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("udo");

    let core = Core::open(&root).await.unwrap();

    assert!(root.join(crate::UDO_FILE_NAME).exists());
    assert!(root.join(DB_FILE_NAME).exists());
    assert!(core.tree().get(&[]).is_some());
}

#[tokio::test]
async fn new_reads_back_the_tree_it_was_given() {
    let (_tmp, tree) = disk_tree().await;

    let core = Core::new(tree, Storage::in_memory());

    assert_eq!(core.tree().get(&[0]).unwrap().name(), "a");
    assert_eq!(core.sessions().running().await.unwrap(), None);
}

// ---------- warnings ----------

#[tokio::test]
async fn warnings_are_taken_once_oldest_first() {
    let (_tmp, core) = core().await;
    assert!(core.take_warnings().is_empty()); // a fresh Core has none

    core.warn("estimate not recorded: disk full");
    core.warn(format!("estimate not recorded: {}", "locked"));

    assert_eq!(
        core.take_warnings(),
        vec![
            "estimate not recorded: disk full",
            "estimate not recorded: locked"
        ]
    );
    assert!(core.take_warnings().is_empty()); // taken: gone
}

// ---------- sessions_of (core: root: [a, ws: [b]]) ----------

/// Task names of `sessions_of(path)`, in its order.
async fn sessions_of(core: &Core, path: &[usize]) -> Vec<String> {
    let sessions = core.sessions_of(path).await.unwrap();
    sessions.into_iter().map(|s| s.task.name).collect()
}

/// Finished sessions a: 9-10, b: 11-12, a: 13-14.
async fn core_with_sessions() -> (tempfile::TempDir, Core) {
    let (tmp, core) = core().await;
    core.add_session(&[0], at(9, 0), at(10, 0), at(20, 0))
        .await
        .unwrap();
    core.add_session(&[1, 0], at(11, 0), at(12, 0), at(20, 0))
        .await
        .unwrap();
    core.add_session(&[0], at(13, 0), at(14, 0), at(20, 0))
        .await
        .unwrap();
    (tmp, core)
}

#[tokio::test]
async fn sessions_of_a_task_are_only_its_own_oldest_first() {
    let (_tmp, core) = core_with_sessions().await;

    let a = core.sessions_of(&[0]).await.unwrap();

    let starts: Vec<_> = a.iter().map(|s| s.start).collect();
    assert_eq!(starts, vec![at(9, 0), at(13, 0)]);
    assert!(a.iter().all(|s| s.task.name == "a"));
}

#[tokio::test]
async fn sessions_of_a_container_are_those_of_the_tasks_below() {
    let (_tmp, core) = core_with_sessions().await;

    assert_eq!(sessions_of(&core, &[1]).await, vec!["b"]); // "ws": not "a"
    assert_eq!(sessions_of(&core, &[]).await, vec!["a", "b", "a"]); // the root
}

#[tokio::test]
async fn sessions_of_leave_removed_ones_out() {
    let (_tmp, core) = core().await;
    let s = core
        .add_session(&[0], at(9, 0), at(10, 0), at(20, 0))
        .await
        .unwrap();

    core.delete_session(s.id).await.unwrap();

    assert!(core.sessions_of(&[0]).await.unwrap().is_empty());
}

#[tokio::test]
async fn sessions_of_include_a_running_one() {
    let (_tmp, mut core) = core().await;

    core.start(&[0], at(9, 0)).await.unwrap();

    let a = core.sessions_of(&[0]).await.unwrap();
    assert_eq!(a.len(), 1);
    assert_eq!(a[0].end, None);
}

#[tokio::test]
async fn sessions_of_a_missing_node_is_empty() {
    let (_tmp, core) = core_with_sessions().await;

    assert!(core.sessions_of(&[9]).await.unwrap().is_empty());
}
