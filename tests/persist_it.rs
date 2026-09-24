use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use udo::persist::write_toml_atomic;

#[derive(Debug, Serialize, Deserialize, PartialEq)]
struct Sample {
    name: String,
    count: u32,
}

fn count_entries(dir: &Path) -> usize {
    fs::read_dir(dir).unwrap().count()
}

// Replicates main()'s context: #[tokio::main] is multi-thread block_on.
#[tokio::main(flavor = "multi_thread")]
async fn run_write(path: &Path, data: &Sample) -> Result<(), Box<dyn std::error::Error>> {
    // does tokio::task::id() panic here (root block_on task)?
    write_toml_atomic(path, data).await
}

#[test]
fn works_in_block_on_root() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(".udo.toml");
    let data = Sample { name: "algorithms".into(), count: 3 };

    run_write(&path, &data).unwrap();

    let read: Sample = toml::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(read, data);
    assert_eq!(count_entries(dir.path()), 1, "leaked temp file(s)");
}

#[tokio::test]
async fn overwrites_and_no_temp_leak() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(".udo.toml");

    write_toml_atomic(&path, &Sample { name: "a".into(), count: 1 }).await.unwrap();
    write_toml_atomic(&path, &Sample { name: "b".into(), count: 2 }).await.unwrap();

    let read: Sample = toml::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(read, Sample { name: "b".into(), count: 2 });
    assert_eq!(count_entries(dir.path()), 1, "leaked temp file(s)");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_writers_never_corrupt() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(".udo.toml");

    let mut handles = Vec::new();
    for i in 0..16u32 {
        let p = path.clone();
        handles.push(tokio::spawn(async move {
            write_toml_atomic(&p, &Sample { name: format!("w{i}"), count: i }).await.unwrap();
        }));
    }
    for h in handles {
        h.await.unwrap();
    }

    let read: Sample = toml::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert!(read.count < 16);
    assert_eq!(count_entries(dir.path()), 1, "leaked temp file(s)");
}
