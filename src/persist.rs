use std::error::Error;
use std::io::Write;
use std::path::Path;

use serde::Serialize;
use tempfile::NamedTempFile;

/// Serialize `data` to TOML and write it to `path` atomically and durably,
/// cross-platform.
///
/// Strategy:
/// 1. Write to a temp file in the *same directory* as the target. Same dir =>
///    same filesystem, and rename is only atomic within one filesystem.
/// 2. `sync_all` (fsync) forces the bytes to disk before the rename, so a power
///    loss can't leave a renamed-but-empty file.
/// 3. `NamedTempFile::persist` performs the OS atomic replace primitive:
///    `rename(2)` on unix, `ReplaceFileW` / `MoveFileExW(REPLACE_EXISTING)` on
///    Windows. It replaces an existing target on both.
/// 4. On unix, fsync the parent directory so the rename itself is durable.
///
/// A crash at any point leaves either the old file or the new file, never a
/// truncated one. The temp file's random name avoids collisions between
/// concurrent writers, and it is auto-removed on any early return.
pub async fn write_toml_atomic<T: Serialize>(path: &Path, data: &T) -> Result<(), Box<dyn Error>> {
    let toml_string = toml::to_string_pretty(data)?;
    let path = path.to_path_buf();

    // File I/O is blocking; keep it off the async runtime. Files are tiny.
    tokio::task::spawn_blocking(move || -> Result<(), Box<dyn Error + Send + Sync>> {
        let dir = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .ok_or("target path has no parent directory")?;

        // temp file in the SAME directory as the target (random unique name)
        let mut tmp = NamedTempFile::new_in(dir)?;
        tmp.write_all(toml_string.as_bytes())?;
        tmp.flush()?; // flush user-space buffer
        tmp.as_file().sync_all()?; // fsync: bytes on disk before rename

        // atomic replace (rename on unix, ReplaceFile/MoveFileEx on Windows)
        tmp.persist(&path)?;

        // persist the rename itself in the directory entry (unix)
        #[cfg(unix)]
        {
            let dir_file = std::fs::File::open(dir)?;
            dir_file.sync_all()?;
        }

        Ok(())
    })
    .await
    .map_err(|e| -> Box<dyn Error> { Box::new(e) })? // JoinError -> Box<dyn Error>
    .map_err(|e| -> Box<dyn Error> { e })?; // drop Send + Sync bounds

    Ok(())
}

#[cfg(test)]
mod tests {
    use serde::{Deserialize, Serialize};
    use std::fs;
    use std::path::Path;

    use crate::persist::write_toml_atomic;

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
        let data = Sample {
            name: "algorithms".into(),
            count: 3,
        };

        run_write(&path, &data).unwrap();

        let read: Sample = toml::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(read, data);
        assert_eq!(count_entries(dir.path()), 1, "leaked temp file(s)");
    }

    #[tokio::test]
    async fn overwrites_and_no_temp_leak() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".udo.toml");

        write_toml_atomic(
            &path,
            &Sample {
                name: "a".into(),
                count: 1,
            },
        )
        .await
        .unwrap();
        write_toml_atomic(
            &path,
            &Sample {
                name: "b".into(),
                count: 2,
            },
        )
        .await
        .unwrap();

        let read: Sample = toml::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(
            read,
            Sample {
                name: "b".into(),
                count: 2
            }
        );
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
                write_toml_atomic(
                    &p,
                    &Sample {
                        name: format!("w{i}"),
                        count: i,
                    },
                )
                .await
                .unwrap();
            }));
        }
        for h in handles {
            h.await.unwrap();
        }

        let read: Sample = toml::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert!(read.count < 16);
        assert_eq!(count_entries(dir.path()), 1, "leaked temp file(s)");
    }
}
