use std::{path::Path, process::ExitStatus};

use crate::run::{RunContext, RunError, Stdout, child::wait, context::TASK_VARS};

/// Run `script` for `ctx`: in its working folder, with the `UDO_*`
/// variables on top of udo's own environment, the terminal handed over
/// unchanged (no pipes; stdout to stderr for `--json`, see `Stdout`) and
/// Ctrl+C left to the script. Returns when the script does; what its exit
/// code means is the caller's business (the TUI asks for Enter first, the
/// CLI passes it on).
pub async fn launch(
    script: &Path,
    ctx: &RunContext,
    stdout: Stdout,
) -> Result<ExitStatus, RunError> {
    let launch_error = |error| RunError::Launch {
        path: script.to_path_buf(),
        error,
    };
    let bin = std::env::current_exe().map_err(launch_error)?;
    let mut cmd = command(script, ctx, &bin);
    stdout.apply(&mut cmd).map_err(launch_error)?;
    wait(cmd).await.map_err(launch_error)
}

/// The command `launch` runs, built apart so tests can look at it.
fn command(script: &Path, ctx: &RunContext, bin: &Path) -> tokio::process::Command {
    let mut cmd = tokio::process::Command::new(script);
    cmd.current_dir(ctx.working_dir());
    if ctx.task.is_none() {
        // run from inside another run (a script calling udo): no stale
        // task from the outer one
        for key in TASK_VARS {
            cmd.env_remove(key);
        }
    }
    cmd.envs(ctx.env(bin));
    cmd
}

#[cfg(test)]
mod tests {
    use std::{ffi::OsStr, fs, os::unix::fs::PermissionsExt, path::PathBuf};

    use super::*;
    use crate::{
        run::{Event, RunContext},
        test_util::disk_tree, // root (tmp): [a, ws (tmp/ws): [b]]
    };

    /// An executable script `body` in `dir`.
    fn script(dir: &Path, body: &str) -> PathBuf {
        let path = dir.join("script");
        fs::write(&path, body).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    /// The script writes its `UDO_*` variables and folder into `out`.
    #[tokio::test]
    async fn the_script_gets_the_context_and_its_folder() {
        let (tmp, tree) = disk_tree().await;
        let scripts = tempfile::tempdir().unwrap();
        let out = scripts.path().join("out");
        let path = script(
            scripts.path(),
            &format!(
                "#!/bin/sh\n{{ env | grep '^UDO_' | sort; pwd -P; }} > '{}'\n",
                out.display()
            ),
        );
        let ctx = RunContext::new(&tree, Event::Open, &[1, 0], Some(&[1, 0])).unwrap();

        let status = launch(&path, &ctx, Stdout::Inherit).await.unwrap();

        assert!(status.success());
        let seen = fs::read_to_string(&out).unwrap();
        let b = tree.get(&[1, 0]).unwrap();
        assert!(seen.contains("UDO_EVENT=open\n"), "{seen}");
        assert!(
            seen.contains(&format!("UDO_TASK_ID={}\n", b.id())),
            "{seen}"
        );
        assert!(seen.contains("UDO_NODE_NAME=b\n"), "{seen}");
        assert!(seen.contains("UDO_BIN=/"), "{seen}");
        // `b` has no folder: the container's (`-P`: /tmp is /private/tmp)
        let ws = tmp.path().join("ws").canonicalize().unwrap();
        assert!(seen.ends_with(&format!("{}\n", ws.display())), "{seen}");
    }

    /// A failing script is no launch error: its code is the caller's.
    #[tokio::test]
    async fn a_failing_script_returns_its_status() {
        let (_tmp, tree) = disk_tree().await;
        let scripts = tempfile::tempdir().unwrap();
        let path = script(scripts.path(), "#!/bin/sh\nexit 3\n");
        let ctx = RunContext::new(&tree, Event::Open, &[0], Some(&[0])).unwrap();

        let status = launch(&path, &ctx, Stdout::Inherit).await.unwrap();

        assert_eq!(status.code(), Some(3));
    }

    /// Redirected for `--json`: runs the same, the code comes back.
    #[tokio::test]
    async fn stdout_on_stderr_runs_the_same() {
        let (_tmp, tree) = disk_tree().await;
        let scripts = tempfile::tempdir().unwrap();
        let path = script(scripts.path(), "#!/bin/sh\necho hi\nexit 3\n");
        let ctx = RunContext::new(&tree, Event::Open, &[0], Some(&[0])).unwrap();

        let status = launch(&path, &ctx, Stdout::Stderr).await.unwrap();

        assert_eq!(status.code(), Some(3));
    }

    /// The script exists, its interpreter does not: say where to look.
    #[tokio::test]
    async fn a_wrong_shebang_points_at_it() {
        let (_tmp, tree) = disk_tree().await;
        let scripts = tempfile::tempdir().unwrap();
        let path = script(scripts.path(), "#!/usr/bin/pyhton\n");
        let ctx = RunContext::new(&tree, Event::Open, &[0], Some(&[0])).unwrap();

        let err = launch(&path, &ctx, Stdout::Inherit).await.unwrap_err();

        assert!(matches!(err, RunError::Launch { .. }));
        assert!(err.to_string().ends_with("(check its #! line)"), "{err}");
    }

    /// Without a task, `UDO_TASK_*` from udo's own environment (a script
    /// that runs udo) are removed, not passed on.
    #[tokio::test]
    async fn without_a_task_inherited_task_variables_are_removed() {
        let (_tmp, tree) = disk_tree().await;
        let ctx = RunContext::new(&tree, Event::Create, &[1], None).unwrap();

        let cmd = command(Path::new("/x"), &ctx, Path::new("/bin/udo"));

        let removed: Vec<&OsStr> = cmd
            .as_std()
            .get_envs() // sorted by name
            .filter(|(_, value)| value.is_none())
            .map(|(key, _)| key)
            .collect();
        let mut expected = TASK_VARS.map(OsStr::new);
        expected.sort();
        assert_eq!(removed, expected);
    }
}
