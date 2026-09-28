use std::{fmt, str::FromStr};

/// What a `session_edits` row records.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditKind {
    Edit,
    Split,
    Cut,
    Delete,
}

impl EditKind {
    /// As stored in `session_edits.kind`.
    fn as_str(self) -> &'static str {
        match self {
            Self::Edit => "edit",
            Self::Split => "split",
            Self::Cut => "cut",
            Self::Delete => "delete",
        }
    }
}

impl fmt::Display for EditKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for EditKind {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "edit" => Ok(Self::Edit),
            "split" => Ok(Self::Split),
            "cut" => Ok(Self::Cut),
            "delete" => Ok(Self::Delete),
            other => Err(format!("unknown edit kind {other:?}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every variant. A new one makes the match below fail to compile, as a
    /// reminder to add it here too.
    const ALL: [EditKind; 4] = [
        EditKind::Edit,
        EditKind::Split,
        EditKind::Cut,
        EditKind::Delete,
    ];

    #[test]
    fn every_kind_parses_back_from_its_name() {
        for kind in ALL {
            match kind {
                EditKind::Edit | EditKind::Split | EditKind::Cut | EditKind::Delete => {}
            }
            assert_eq!(kind.as_str().parse(), Ok(kind));
            assert_eq!(kind.to_string(), kind.as_str());
        }
    }

    #[test]
    fn names_are_distinct() {
        let mut names: Vec<_> = ALL.iter().map(|k| k.as_str()).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), ALL.len());
    }

    #[test]
    fn unknown_names_are_errors() {
        assert!("spilt".parse::<EditKind>().is_err());
        assert!("Edit".parse::<EditKind>().is_err()); // exact, lowercase
        assert!("".parse::<EditKind>().is_err());
    }
}
