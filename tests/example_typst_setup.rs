//! `examples/run/typst-setup.sh` as installed: `on_create` copies the
//! template into a new task's folder, and stays out of everything else.

mod common;

use std::{fs, path::Path};

use common::{install_example, ok, udo};

/// A fresh root with `typst-setup` installed and set as `on_create`, and
/// a template (`main.typ`) at `<root>/templates/typst`.
fn root() -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    ok(root.path(), &["status"]); // creates the root
    install_example(root.path(), "typst-setup");
    let template = root.path().join("templates/typst");
    fs::create_dir_all(&template).unwrap();
    fs::write(template.join("main.typ"), "= Template\n").unwrap();
    ok(
        root.path(),
        &["settings", "set", "/", "on_create=typst-setup"],
    );
    root
}

fn read(path: &Path) -> String {
    fs::read_to_string(path).unwrap()
}

#[test]
fn a_new_task_gets_the_template() {
    let root = root();
    let dir = root.path().join("lab 1");

    let out = ok(
        root.path(),
        &["add", "task", "lab 1", "--dir", dir.to_str().unwrap()],
    );

    assert_eq!(read(&dir.join("main.typ")), "= Template\n");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("typst-setup: lab 1 ready"), "{out:?}");
    assert!(stdout.contains("on_create: typst-setup"), "{out:?}");
}

/// `-n`: what the folder already has stays.
#[test]
fn files_already_there_are_kept() {
    let root = root();
    let dir = root.path().join("lab 1");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("main.typ"), "mine\n").unwrap();

    ok(
        root.path(),
        &["add", "task", "lab 1", "--dir", dir.to_str().unwrap()],
    );

    assert_eq!(read(&dir.join("main.typ")), "mine\n");
}

/// Inherited by everything below: quiet where it does not apply.
#[test]
fn containers_and_tasks_without_a_folder_are_left_alone() {
    let root = root();

    let project = ok(root.path(), &["add", "project", "cs"]);
    let no_dir = ok(root.path(), &["add", "task", "notes", "--no-dir"]);

    assert!(!root.path().join("cs/main.typ").exists());
    for out in [project, no_dir] {
        assert!(
            !String::from_utf8_lossy(&out.stderr).contains("warning"),
            "{out:?}"
        );
    }
}

/// Opened (`udo run --with`): not its event, nothing happens.
#[test]
fn opening_does_nothing() {
    let root = root();
    let dir = root.path().join("lab 1");
    ok(
        root.path(),
        &[
            "add",
            "task",
            "lab 1",
            "--dir",
            dir.to_str().unwrap(),
            "--no-run",
        ],
    );

    let out = ok(root.path(), &["run", "lab 1", "--with", "typst-setup"]);

    assert!(!dir.join("main.typ").exists());
    assert!(out.stdout.is_empty(), "{out:?}");
}

/// Set up but no template: a mistake, so a warning; the task is there.
#[test]
fn without_a_template_add_warns_and_keeps_the_task() {
    let root = root();
    fs::remove_dir_all(root.path().join("templates")).unwrap();
    let dir = root.path().join("lab 1");

    let out = udo(
        root.path(),
        &["add", "task", "lab 1", "--dir", dir.to_str().unwrap()],
    );

    assert!(out.status.success(), "{out:?}");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("typst-setup: no template at"), "{stderr}");
    assert!(
        stderr.contains("warning: on_create typst-setup: exited with 1"),
        "{stderr}"
    );
    assert!(String::from_utf8_lossy(&ok(root.path(), &["ls"]).stdout).contains("lab 1"));
}
