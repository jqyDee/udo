use std::{fmt, str::FromStr};

/// What kind of session: by hand, or which program recorded it. One per
/// kind (two IntelliJ windows are both `idea`); who may stop a session is
/// its `Owner`, one per instance. Kept forever (statistics, learning by
/// program); the owner only matters while a session runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionSource {
    /// `s` in the TUI, `udo start`, sessions added by hand.
    Manual,
    /// A program via `udo track` (`tmux`, `idea`, …): a-z 0-9 -, not empty,
    /// not `manual`. Checked by `FromStr` (CLI and database both parse);
    /// built directly only for names known to be valid.
    Program(String),
}

impl SessionSource {
    /// How it is written in files and the database; `FromStr` reads it back.
    pub fn as_str(&self) -> &str {
        match self {
            Self::Manual => "manual",
            Self::Program(name) => name,
        }
    }
}

impl fmt::Display for SessionSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for SessionSource {
    type Err = String;

    /// `manual` or a program name. Exact: `Manual` or `Tmux` are errors.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "manual" => Ok(Self::Manual),
            name if is_program_name(name) => Ok(Self::Program(name.to_owned())),
            other => Err(format!("invalid session source {other:?}: use a-z, 0-9, -")),
        }
    }
}

/// Not empty, only a-z 0-9 -. `manual` passes here too: `from_str` catches
/// it first.
fn is_program_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One of each variant. A new one makes the match below fail to
    /// compile, as a reminder to add it here too.
    fn all() -> Vec<SessionSource> {
        vec![SessionSource::Manual, SessionSource::Program("tmux".into())]
    }

    #[test]
    fn every_source_parses_back_from_its_name() {
        for source in all() {
            match source {
                SessionSource::Manual | SessionSource::Program(_) => {} // exhaustive: see `all`
            }
            assert_eq!(source.as_str().parse(), Ok(source.clone()));
            assert_eq!(source.to_string(), source.as_str());
        }
    }

    #[test]
    fn program_names_parse() {
        assert_eq!("emacs".parse::<SessionSource>(), Ok(SessionSource::Program("emacs".into())));
        assert_eq!(
            "nvim-tmux".parse::<SessionSource>(),
            Ok(SessionSource::Program("nvim-tmux".into()))
        );
        assert_eq!("idea2".parse::<SessionSource>(), Ok(SessionSource::Program("idea2".into())));
    }

    #[test]
    fn unknown_names_are_errors() {
        for bad in ["", "Manual", "Tmux", "my tool", "idea!", "tmux:x"] {
            // exact, lowercase; `:` belongs to owners (`tmux:udo-…`)
            assert!(bad.parse::<SessionSource>().is_err(), "{bad:?}");
        }
    }
}
