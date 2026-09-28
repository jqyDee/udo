use std::{fmt, str::FromStr};

use uuid::Uuid;

/// Stable ID of a work session. v7 like `NodeId`: time-ordered, and safe to
/// copy between backends.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SessionId(Uuid);

impl SessionId {
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        Self(Uuid::now_v7())
    }
}

impl fmt::Display for SessionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl FromStr for SessionId {
    type Err = uuid::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        s.parse().map(Self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_back_from_its_text() {
        let id = SessionId::new();
        assert_eq!(id.to_string().parse::<SessionId>().unwrap(), id);
    }

    #[test]
    fn garbage_is_an_error() {
        assert!("not a uuid".parse::<SessionId>().is_err());
        assert!("".parse::<SessionId>().is_err());
    }

    #[test]
    fn new_ids_sort_by_creation() {
        // v7: time-ordered, so text order = creation order (a good DB key)
        let (a, b) = (SessionId::new(), SessionId::new());
        assert!(a.to_string() < b.to_string());
    }
}
