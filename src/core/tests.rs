use crate::{
    core::Core,
    model::sessions::SessionStore,
    storage::{Storage, sqlite::DB_FILE_NAME},
    test_util::disk_tree,
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
