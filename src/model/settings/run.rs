//! Run config names and the settings that pick one (`open_with`,
//! `on_create`). Here, not in `run`, because `ContainerSettings` holds them
//! and `model` does not depend on `run`.

use std::{fmt, str::FromStr};

use serde::{Deserialize, Serialize};

/// A run config's name: the file stem of its script (`idea.py` -> `idea`),
/// so the language can change without touching any `.udo.toml`. Letters,
/// digits, `-`, `_`; not `none` (that switches a setting off). The field is
/// private: `FromStr` is the way in.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RunName(String);

impl RunName {
    /// The word that switches `open_with` / `on_create` off.
    const OFF: &str = "none";

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for RunName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for RunName {
    type Err = String;

    /// Exact (no trimming): file stems are compared as they are.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s == Self::OFF {
            return Err(format!("{s:?} switches the setting off, it cannot name a script"));
        }
        let allowed = |c: char| c.is_ascii_alphanumeric() || c == '-' || c == '_';
        if s.is_empty() || !s.chars().all(allowed) {
            return Err(format!("invalid run config name {s:?}: use letters, digits, -, _"));
        }
        Ok(Self(s.to_owned()))
    }
}

/// `open_with` / `on_create`: a script by name, or switched off (`none`: a
/// child can switch off what its parent switched on). Written as text in
/// `.udo.toml`; an unknown name loads fine and only fails when run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub enum RunSetting {
    Off,
    Script(RunName),
}

impl RunSetting {
    /// The script to run; `Off`: none.
    pub fn script(&self) -> Option<&RunName> {
        match self {
            Self::Off => None,
            Self::Script(name) => Some(name),
        }
    }
}

impl fmt::Display for RunSetting {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Off => f.write_str(RunName::OFF),
            Self::Script(name) => name.fmt(f),
        }
    }
}

impl FromStr for RunSetting {
    type Err = String;

    /// `none` or a name; surrounding whitespace is ignored (typed text).
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim() {
            RunName::OFF => Ok(Self::Off),
            name => name.parse().map(Self::Script),
        }
    }
}

/// For serde (`try_from`): the file's text.
impl TryFrom<String> for RunSetting {
    type Error = String;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        s.parse()
    }
}

/// For serde (`into`): as written in the file.
impl From<RunSetting> for String {
    fn from(s: RunSetting) -> Self {
        s.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::settings::ContainerSettings;

    #[test]
    fn names_parse_back_from_their_text() {
        for text in ["nvim-tmux", "typst_setup", "Idea2", "x"] {
            let name: RunName = text.parse().unwrap();
            assert_eq!((name.as_str(), name.to_string()), (text, text.to_string()));
        }
    }

    #[test]
    fn invalid_names_are_errors() {
        for bad in ["", "none", "my script", "a/b", "idea.py", " idea", "über"] {
            assert!(bad.parse::<RunName>().is_err(), "{bad:?}");
        }
    }

    #[test]
    fn none_is_off_anything_else_a_script() {
        assert_eq!("none".parse(), Ok(RunSetting::Off));
        assert_eq!(" none ".parse(), Ok(RunSetting::Off)); // typed text
        let idea = RunSetting::Script("idea".parse().unwrap());
        assert_eq!(" idea ".parse(), Ok(idea.clone()));
        assert_eq!(idea.script().map(RunName::as_str), Some("idea"));
        assert_eq!(RunSetting::Off.script(), None);
        assert!("my script".parse::<RunSetting>().is_err());
    }

    #[test]
    fn settings_display_like_they_parse() {
        for text in ["none", "nvim-tmux"] {
            assert_eq!(text.parse::<RunSetting>().unwrap().to_string(), text);
        }
    }

    #[test]
    fn written_as_text_in_the_file() {
        let s = ContainerSettings {
            open_with: Some("nvim-tmux".parse().unwrap()),
            on_create: Some(RunSetting::Off),
            ..Default::default()
        };

        let text = toml::to_string(&s).unwrap();

        assert!(text.contains("open_with = \"nvim-tmux\""), "{text}");
        assert!(text.contains("on_create = \"none\""), "{text}");
        assert_eq!(toml::from_str::<ContainerSettings>(&text).unwrap(), s);
    }

    #[test]
    fn an_invalid_name_in_the_file_is_a_load_error() {
        assert!(toml::from_str::<ContainerSettings>("open_with = \"my script\"").is_err());
    }
}
