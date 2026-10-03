//! Every script in `examples/run/`: valid bash (`bash -n`, always) and
//! clean under `shellcheck` (if installed; else skipped with a note).
//! Their Python twins in `examples/python/` (for comparison): valid
//! Python and executable.

mod common;

use std::{fs, path::PathBuf, process::Command};

use common::have;

fn scripts() -> Vec<PathBuf> {
    files_in("examples/run")
}

/// The files in `dir` (below the crate), sorted; at least one.
fn files_in(dir: &str) -> Vec<PathBuf> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(dir);
    let mut scripts: Vec<PathBuf> = fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    scripts.sort();
    assert!(!scripts.is_empty(), "no examples");
    scripts
}

#[test]
fn the_examples_are_valid_bash() {
    for script in scripts() {
        let out = Command::new("bash")
            .arg("-n")
            .arg(&script)
            .output()
            .unwrap();
        assert!(out.status.success(), "{}: {out:?}", script.display());
    }
}

/// Executable, as `Library` needs them (copying keeps the mode).
#[test]
fn the_examples_are_executable() {
    use std::os::unix::fs::PermissionsExt;
    for script in scripts() {
        let mode = fs::metadata(&script).unwrap().permissions().mode();
        assert!(mode & 0o111 != 0, "{} is not executable", script.display());
    }
}

#[test]
fn the_examples_pass_shellcheck() {
    if !have("shellcheck") {
        eprintln!("skipped: no shellcheck (brew install shellcheck)");
        return;
    }
    let out = Command::new("shellcheck").args(scripts()).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stdout));
}

/// One Python twin per example, by the same name: none forgotten.
#[test]
fn every_example_has_a_python_twin() {
    let stems = |files: Vec<PathBuf>| -> Vec<String> {
        let stem = |p: &PathBuf| p.file_stem().unwrap().to_string_lossy().into_owned();
        files.iter().map(stem).collect()
    };
    assert_eq!(stems(files_in("examples/python")), stems(scripts()));
}

/// Parsed, not compiled: `py_compile` would leave `__pycache__` in the
/// examples.
#[test]
fn the_python_twins_are_valid_python_and_executable() {
    use std::os::unix::fs::PermissionsExt;
    if !have("python3") {
        eprintln!("skipped: no python3");
        return;
    }
    for script in files_in("examples/python") {
        let out = Command::new("python3")
            .args(["-c", "import ast, sys; ast.parse(open(sys.argv[1]).read())"])
            .arg(&script)
            .output()
            .unwrap();
        assert!(out.status.success(), "{}: {out:?}", script.display());
        let mode = fs::metadata(&script).unwrap().permissions().mode();
        assert!(mode & 0o111 != 0, "{} is not executable", script.display());
    }
}
