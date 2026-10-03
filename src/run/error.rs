use std::{fmt, io, path::PathBuf};

use crate::model::settings::RunName;

/// Why a run config cannot be used. The texts say how to fix it: the CLI
/// prints them, the TUI shows them as toasts.
#[derive(Debug)]
pub enum RunError {
    /// `no run config "idae" in /…/run (have: idea, nvim-tmux)`
    NotFound {
        name: RunName,
        dir: PathBuf,
        have: Vec<RunName>,
    },
    /// Two executable files with the same stem (`idea.sh`, `idea.py`).
    /// `paths` sorted, so the message does not depend on `read_dir`.
    Duplicate { name: RunName, paths: Vec<PathBuf> },
    /// The file is there but lacks `+x`.
    NotExecutable { path: PathBuf },
    /// The folder exists but cannot be read.
    Io { dir: PathBuf, error: io::Error },
    /// The script was found but does not start.
    Launch { path: PathBuf, error: io::Error },
}

impl fmt::Display for RunError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound { name, dir, have } => {
                write!(f, "no run config {:?} in {} ", name.as_str(), dir.display())?;
                if have.is_empty() {
                    write!(f, "(none yet)")
                } else {
                    let names: Vec<&str> = have.iter().map(RunName::as_str).collect();
                    write!(f, "(have: {})", names.join(", "))
                }
            }
            Self::Duplicate { name, paths } => {
                let files: Vec<String> = paths
                    .iter()
                    .map(|p| {
                        p.file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .into_owned()
                    })
                    .collect();
                write!(
                    f,
                    "two run configs named {:?}: {} (rename one)",
                    name.as_str(),
                    files.join(", ")
                )
            }
            Self::NotExecutable { path } => {
                let name = path.file_stem().unwrap_or_default().to_string_lossy();
                write!(f, "run config {name:?} is not executable: chmod +x {}", path.display())
            }
            Self::Io { dir, error } => write!(f, "cannot read {}: {error}", dir.display()),
            Self::Launch { path, error } => {
                write!(f, "cannot run {}: {error}", path.display())?;
                match error.kind() {
                    // the script is there: its interpreter is not
                    io::ErrorKind::NotFound => write!(f, " (check its #! line)"),
                    io::ErrorKind::PermissionDenied => {
                        write!(f, " (chmod +x {})", path.display())
                    }
                    _ => Ok(()),
                }
            }
        }
    }
}

impl std::error::Error for RunError {}
