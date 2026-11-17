use std::path::Path;

use clap::Parser;
use udo::{
    Res,
    cli::Cli,
    core::Core,
    dir::{Dirs, default_data, default_root, dirs, is_same_root},
    storage::relocate::move_old_db,
};

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    let result = async {
        let dirs = dirs()?;
        if cfg!(debug_assertions) {
            refuse_real_dirs(&dirs, &default_root()?, &default_data()?)?;
        }
        // before anything opens a database (#93)
        if let Some(moved) = move_old_db(&dirs)? {
            eprintln!(
                "moved udo.db to {}; old copy kept at {} (delete it once you are happy)",
                dirs.data.display(),
                moved.kept.display()
            );
        }
        let mut core = Core::open(&dirs).await?;
        cli.execute(&mut core, &std::env::current_dir()?).await
    }
    .await;

    if let Err(e) = result {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

/// A debug build never opens the real data, however it was started or
/// spelled (`is_same_root`: also a symlink, `..`): neither the real root
/// (`~/.config/udo`, the tree) nor the real data dir (`udo.db`), and it
/// never moves one. `cargo run` sets only `UDO_ROOT` (`.cargo/config.toml`:
/// a custom root keeps `udo.db` in it); a `target/debug/udo` started by
/// hand or by a hook needs a test `UDO_ROOT` too. An exported real
/// `UDO_DATA` is refused. Either real dir counts in either slot: before
/// the move the real `udo.db` sits in the real root. Release builds (the
/// installed udo) skip the check.
fn refuse_real_dirs(dirs: &Dirs, real_root: &Path, real_data: &Path) -> Res<()> {
    for dir in [&dirs.root, &dirs.data] {
        if [real_root, real_data]
            .into_iter()
            .any(|real| is_same_root(dir, real))
        {
            return Err(format!(
                "a debug build refuses the real data at {}: point {} and {} at a test dir \
                 (scripts/seed-testdata.sh makes /tmp/udo-test)",
                dir.display(),
                udo::ROOT_ENV,
                udo::DATA_ENV
            )
            .into());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROOT: &str = "/home/t/.config/udo";
    const DATA: &str = "/home/t/data/udo";

    fn check(root: &str, data: &str) -> Result<(), String> {
        let dirs = Dirs {
            root: root.into(),
            data: data.into(),
            move_old_db: false,
        };
        refuse_real_dirs(&dirs, Path::new(ROOT), Path::new(DATA)).map_err(|e| e.to_string())
    }

    #[test]
    fn the_real_root_is_refused() {
        let err = check(ROOT, "/tmp/d").unwrap_err();
        assert!(err.contains(ROOT), "{err}");
    }

    /// `cargo run` with a test root, but `UDO_DATA` exported to the real
    /// data dir in the shell.
    #[test]
    fn the_real_data_dir_is_refused_also_with_a_test_root() {
        let err = check("/tmp/r", DATA).unwrap_err();
        assert!(err.contains(DATA), "{err}");
        assert!(err.contains("UDO_ROOT and UDO_DATA"), "{err}");
    }

    /// Before the move the real udo.db lives in the real root.
    #[test]
    fn the_real_root_as_data_dir_is_refused() {
        let err = check("/tmp/r", ROOT).unwrap_err();
        assert!(err.contains(ROOT), "{err}");
        assert!(err.contains("UDO_ROOT and UDO_DATA"), "{err}");
    }

    #[test]
    fn the_real_data_dir_as_root_is_refused() {
        let err = check(DATA, "/tmp/d").unwrap_err();
        assert!(err.contains(DATA), "{err}");
    }

    #[test]
    fn test_dirs_pass() {
        assert_eq!(check("/tmp/r", "/tmp/r"), Ok(()));
    }
}
