use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionError {
    NotFound,
    EndBeforeStart,
    Overlap,
    AlreadyRunning,
    /// `split` / `cut` time not inside the session.
    OutsideSession,
    /// `cut` would remove everything (use `delete`).
    WholeSession,
    Running,
    /// Database error, as text (keeps the enum `Send + Sync + PartialEq`).
    Backend(String),
}

impl fmt::Display for SessionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound => write!(f, "session not found"),
            Self::EndBeforeStart => write!(f, "end must be after start"),
            Self::Overlap => write!(f, "overlaps another session"),
            Self::AlreadyRunning => write!(f, "another session is already running"),
            Self::OutsideSession => write!(f, "time outside of session time"),
            Self::WholeSession => write!(f, "would remove the whole session, delete it instead"),
            Self::Running => write!(f, "session is still running, stop it first"),
            Self::Backend(e) => write!(f, "session storage: {e}"),
        }
    }
}

impl std::error::Error for SessionError {}
