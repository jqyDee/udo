use crate::{
    core::Core,
    dir::Dirs,
    model::{
        node::{HeaderPatch, NodePatch},
        sessions::{Owner, SessionPatch, SessionStore},
        settings::ContainerSettings,
    },
    storage::{Storage, sqlite::DB_FILE_NAME},
    test_util::{at, core, disk_tree, task},
};

#[tokio::test]
async fn open_creates_the_root_and_the_database_in_the_data_dir() {
    let tmp = tempfile::tempdir().unwrap();
    let dirs = Dirs {
        root: tmp.path().join("udo"),
        data: tmp.path().join("data").join("udo"), // parent missing too
        move_old_db: false,
    };

    let core = Core::open(&dirs).await.unwrap();

    assert!(dirs.root.join(crate::UDO_FILE_NAME).exists());
    assert!(dirs.data.join(DB_FILE_NAME).exists());
    assert!(!dirs.root.join(DB_FILE_NAME).exists());
    assert_eq!(core.data_dir(), dirs.data);
    assert!(core.tree().get(&[]).is_some());
}

/// In-use fallback: the db stays in the root for this run, but scripts
/// and hooks still learn the real data dir (`UDO_DATA`).
#[tokio::test]
async fn open_db_in_keeps_the_data_dir_for_scripts() {
    let tmp = tempfile::tempdir().unwrap();
    let dirs = Dirs {
        root: tmp.path().join("udo"),
        data: tmp.path().join("data"),
        move_old_db: true,
    };

    let core = Core::open_db_in(&dirs, &dirs.root).await.unwrap();

    assert!(dirs.root.join(DB_FILE_NAME).exists());
    assert!(!dirs.data.join(DB_FILE_NAME).exists());
    assert_eq!(core.data_dir(), dirs.data);
}

/// A custom root keeps everything in one dir, as before #93.
#[tokio::test]
async fn open_with_data_in_the_root_keeps_both_there() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("udo");
    let dirs = Dirs {
        root: root.clone(),
        data: root.clone(),
        move_old_db: false,
    };

    Core::open(&dirs).await.unwrap();

    assert!(root.join(crate::UDO_FILE_NAME).exists());
    assert!(root.join(DB_FILE_NAME).exists());
}

#[tokio::test]
async fn new_reads_back_the_tree_it_was_given() {
    let (_tmp, tree) = disk_tree().await;

    let core = Core::new(tree, Storage::in_memory());

    assert_eq!(core.tree().get(&[0]).unwrap().name(), "a");
    assert_eq!(core.sessions().running().await.unwrap(), None);
    assert_eq!(core.data_dir(), core.tree().root.dir().unwrap());
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

// ---------- changes (the TUI rebuilds only when they moved, #61) ----------

/// `core.changes()`, which must have moved since `before` (`what` did it).
fn moved(core: &Core, before: u64, what: &str) -> u64 {
    let now = core.changes();
    assert!(now > before, "{what} is no change");
    now
}

#[tokio::test]
async fn every_write_counts_as_a_change() {
    let (_tmp, mut core) = core().await;
    let mut n = core.changes();

    let c = core.create(&[1], task("c")).await.unwrap();
    n = moved(&core, n, "create");
    let rename = NodePatch {
        header: HeaderPatch {
            name: Some("c2".into()),
            ..Default::default()
        },
        ..Default::default()
    };
    core.edit(&c, rename).await.unwrap();
    n = moved(&core, n, "edit");
    core.set_settings(&[1], ContainerSettings::default(), None)
        .await
        .unwrap();
    n = moved(&core, n, "set_settings");

    let s = core
        .add_session(&[1, 0], at(9, 0), at(11, 0), at(12, 0))
        .await
        .unwrap();
    n = moved(&core, n, "add_session");
    let patch = SessionPatch {
        end: Some(at(10, 30)),
        ..Default::default()
    };
    core.edit_session(s.id, patch, at(12, 0)).await.unwrap();
    n = moved(&core, n, "edit_session");
    let (first, _) = core.split_session(s.id, at(10, 0)).await.unwrap().unwrap();
    n = moved(&core, n, "split_session");
    let left = core
        .cut_session(first.id, at(9, 15), at(9, 30), at(12, 0))
        .await
        .unwrap();
    n = moved(&core, n, "cut_session");
    core.delete_session(left[0].id).await.unwrap();
    n = moved(&core, n, "delete_session");

    core.start(&[0], at(13, 0)).await.unwrap();
    n = moved(&core, n, "start");
    core.stop(at(13, 30)).await.unwrap();
    n = moved(&core, n, "stop");
    let owner: Owner = "tmux:a".parse().unwrap();
    core.track_start(&[0], "tmux".parse().unwrap(), owner.clone(), at(14, 0))
        .await
        .unwrap();
    n = moved(&core, n, "track_start");
    core.track_stop(&owner, at(14, 30)).await.unwrap();
    n = moved(&core, n, "track_stop");

    core.set_done(&[1, 0], true, at(15, 0)).await.unwrap();
    n = moved(&core, n, "set_done");
    core.done_at(&[0], at(15, 0), at(16, 0), false)
        .await
        .unwrap();
    n = moved(&core, n, "done_at");
    core.delete(&c, at(15, 0)).await.unwrap();
    n = moved(&core, n, "delete");
    let plan = core.purge_plan(&[1]).unwrap().unwrap();
    core.purge(&plan, |_| Ok(()), at(15, 0)).await.unwrap();
    moved(&core, n, "purge");
}

#[tokio::test]
async fn reads_are_no_change() {
    let (_tmp, core) = core().await;
    let before = core.changes();

    core.history(at(12, 0)).await.unwrap();
    core.estimates(at(12, 0)).await.unwrap();
    core.estimate(&[0], at(12, 0)).await.unwrap();
    core.totals(&[1], at(12, 0)).await.unwrap();
    core.sessions_of(&[1]).await.unwrap();
    core.running_session().await.unwrap();
    core.tasks_with_sessions().await.unwrap();
    core.purge_plan(&[1]).unwrap();
    core.task_defaults(&[1], at(12, 0).naive_local());

    assert_eq!(core.changes(), before);
}

/// A failed write may have changed part of the data: it counts too.
#[tokio::test]
async fn a_refused_write_still_counts() {
    let (_tmp, mut core) = core().await;
    let before = core.changes();

    assert!(core.start(&[1], at(9, 0)).await.is_err()); // a container

    moved(&core, before, "a refused start");
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
