use std::{fmt, str::FromStr};

/// Where a session was recorded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionSource {
    Manual,
}

impl SessionSource {
    /// How it is written in files and the database.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Manual => "manual",
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

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "manual" => Ok(Self::Manual),
            other => Err(format!("unknown session source {other:?}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every variant. A new one makes the match below fail to compile, as a
    /// reminder to add it here too.
    const ALL: [SessionSource; 1] = [SessionSource::Manual];

    #[test]
    fn every_source_parses_back_from_its_name() {
        for source in ALL {
            match source {
                SessionSource::Manual => {} // exhaustive: see `ALL`
            }
            assert_eq!(source.as_str().parse(), Ok(source));
            assert_eq!(source.to_string(), source.as_str());
        }
    }

    #[test]
    fn unknown_names_are_errors() {
        assert!("emacs".parse::<SessionSource>().is_err());
        assert!("Manual".parse::<SessionSource>().is_err()); // exact, lowercase
        assert!("".parse::<SessionSource>().is_err());
    }
}
