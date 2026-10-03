use std::{fmt, str::FromStr};

/// Who may stop a session: `manual`, or one program instance, e.g.
/// `tmux:udo-01a0…` (one per instance; the kind is the `SessionSource`).
/// A program's `stop` only ends its own session; manual always wins. Not
/// empty, no whitespace: the field is private, so `manual()` and `FromStr`
/// are the only ways in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Owner(String);

impl Owner {
    const MANUAL: &str = "manual";

    /// The owner of `s` / `udo start`. `udo track` refuses it, so programs
    /// cannot pose as the user.
    pub fn manual() -> Self {
        Self(Self::MANUAL.to_owned())
    }

    /// Exactly `manual` (`Manual` is a valid, non-manual owner).
    pub fn is_manual(&self) -> bool {
        self.0 == Self::MANUAL
    }

    /// How it is written in the database and passed to `udo track`;
    /// `FromStr` reads it back.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Owner {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Owner {
    type Err = String;

    /// Anything not empty and without whitespace (Unicode included); `manual`
    /// too (read back from the database).
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.is_empty() || s.chars().any(char::is_whitespace) {
            return Err(format!("invalid owner {s:?}: not empty, no spaces"));
        }
        Ok(Self(s.to_owned()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn owners_parse_back_from_their_text() {
        for text in ["manual", "tmux:udo-01a0", "idea:/Users/x/lab_3", "über"] {
            let owner: Owner = text.parse().unwrap();
            assert_eq!(owner.as_str(), text);
            assert_eq!(owner.to_string(), text);
        }
    }

    #[test]
    fn manual_is_the_parsed_manual() {
        assert_eq!("manual".parse(), Ok(Owner::manual()));
        assert!(Owner::manual().is_manual());
        assert!(!"tmux:x".parse::<Owner>().unwrap().is_manual());
    }

    #[test]
    fn empty_or_whitespace_is_an_error() {
        for bad in ["", " ", "tmux: x", "a\tb", "a\u{a0}b"] {
            assert!(bad.parse::<Owner>().is_err(), "{bad:?}");
        }
    }
}
