use crossterm::event::{KeyCode, KeyEvent};

use crate::model::{container::ContainerKind, settings::TaskFolderSetting};

/// Options of the `folder` field, taken from `FolderMode` so the order can't
/// drift.
pub const FOLDER_CHOICES: &[&str] = &[
    FolderMode::ALL[0].label(),
    FolderMode::ALL[1].label(),
    FolderMode::ALL[2].label(),
];

/// Folder options of the container form: a container always has a folder,
/// so no `none`. Read back by label (`FolderMode::from_label`), not index.
pub const CONTAINER_FOLDER_CHOICES: &[&str] =
    &[FolderMode::Auto.label(), FolderMode::Custom.label()];

/// Options of the `kind` field: `ContainerKind::CREATABLE` (never root).
/// Read back with `kind_from_label`.
pub const CONTAINER_KIND_CHOICES: &[&str] = &[
    ContainerKind::CREATABLE[0].label(),
    ContainerKind::CREATABLE[1].label(),
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChoiceInput {
    pub options: Vec<&'static str>,
    pub selected: usize,
    /// The first option means "not set" (`unsettable`).
    pub unset: bool,
    /// Shown dim after the options while "not set" is chosen: what applies
    /// then (e.g. the inherited value).
    pub hint: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FolderMode {
    Auto,
    Custom,
    None,
}

impl ChoiceInput {
    /// `options` with the option `selected` chosen (the first one if
    /// `selected` isn't among them).
    pub fn new(options: &'static [&'static str], selected: &str) -> Self {
        Self {
            options: options.to_vec(),
            selected: options.iter().position(|o| *o == selected).unwrap_or(0),
            unset: false,
            hint: None,
        }
    }

    /// `unset_label` (e.g. `inherit`) first, then `options`. `selected`:
    /// None or not among `options` -> the unset option.
    pub fn unsettable(
        unset_label: &'static str,
        options: &'static [&'static str],
        selected: Option<&str>,
    ) -> Self {
        let selected = selected
            .and_then(|s| options.iter().position(|o| *o == s))
            .map_or(0, |i| i + 1);
        Self {
            options: [&[unset_label], options].concat(),
            selected,
            unset: true,
            hint: None,
        }
    }

    pub fn with_hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }

    /// Label of the chosen option.
    pub fn selected_label(&self) -> Option<&'static str> {
        self.options.get(self.selected).copied()
    }

    /// The chosen value: None for the unset option.
    pub fn value(&self) -> Option<&'static str> {
        if self.is_unset() {
            None
        } else {
            self.selected_label()
        }
    }

    /// The unset option is chosen.
    pub fn is_unset(&self) -> bool {
        self.unset && self.selected == 0
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> bool {
        let option_count = self.options.len();
        assert!(option_count > 0, "Choice has no fields");
        let before = self.value();
        match key.code {
            KeyCode::Left | KeyCode::Char('h') => {
                self.selected = (self.selected + option_count - 1) % option_count
            }
            KeyCode::Right | KeyCode::Char('l') => {
                self.selected = (self.selected + 1) % option_count
            }
            _ => {}
        }
        self.value() != before
    }
}

impl FolderMode {
    /// Every mode, in the order the form shows them.
    /// Index = `ChoiceInput::selected`.
    pub const ALL: [Self; 3] = [Self::None, Self::Auto, Self::Custom];

    /// Shown in the form.
    pub const fn label(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Auto => "auto",
            Self::Custom => "custom",
        }
    }

    /// Position in `ALL` (for `ChoiceInput::selected`).
    pub const fn index(self) -> usize {
        match self {
            Self::None => 0,
            Self::Auto => 1,
            Self::Custom => 2,
        }
    }

    /// Mode shown as `label`.
    pub fn from_label(label: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|m| m.label() == label)
    }
}

impl From<TaskFolderSetting> for FolderMode {
    fn from(value: TaskFolderSetting) -> Self {
        match value {
            TaskFolderSetting::Auto => FolderMode::Auto,
            TaskFolderSetting::None => FolderMode::None,
        }
    }
}

/// Creatable container kind shown as `label` in `CONTAINER_KIND_CHOICES`.
pub fn kind_from_label(label: &str) -> Option<ContainerKind> {
    ContainerKind::CREATABLE
        .into_iter()
        .find(|k| k.label() == label)
}

#[cfg(test)]
mod tests {
    use crossterm::event::KeyCode;

    use crate::{
        test_util::press,
        tui::form::choice::{
            CONTAINER_FOLDER_CHOICES, CONTAINER_KIND_CHOICES, ChoiceInput, FOLDER_CHOICES,
            FolderMode, kind_from_label,
        },
    };

    #[test]
    fn folder_mode_index_matches_all() {
        for (i, mode) in FolderMode::ALL.into_iter().enumerate() {
            assert_eq!(mode.index(), i, "{mode:?}");
            assert_eq!(FOLDER_CHOICES[i], mode.label());
        }
    }

    #[test]
    fn every_choice_label_maps_back() {
        for &label in FOLDER_CHOICES.iter().chain(CONTAINER_FOLDER_CHOICES) {
            assert_eq!(FolderMode::from_label(label).unwrap().label(), label);
        }
        for &label in CONTAINER_KIND_CHOICES {
            assert_eq!(kind_from_label(label).unwrap().to_string(), label);
        }
        assert_eq!(FolderMode::from_label("nope"), None);
        assert_eq!(kind_from_label("root"), None); // never created in a form
    }

    #[test]
    fn new_selects_by_label_and_falls_back_to_the_first() {
        let c = ChoiceInput::new(CONTAINER_FOLDER_CHOICES, "custom");
        assert_eq!(c.selected_label(), Some("custom"));
        let c = ChoiceInput::new(CONTAINER_FOLDER_CHOICES, "none"); // not offered
        assert_eq!(c.selected_label(), Some("auto"));
    }

    #[test]
    fn unsettable_puts_the_unset_option_first() {
        let c = ChoiceInput::unsettable("inherit", CONTAINER_FOLDER_CHOICES, None);
        assert_eq!(c.options, ["inherit", "auto", "custom"]);
        assert!(c.is_unset());
        assert_eq!(c.value(), None);

        let c = ChoiceInput::unsettable("inherit", CONTAINER_FOLDER_CHOICES, Some("custom"));
        assert_eq!(c.selected_label(), Some("custom"));
        assert!(!c.is_unset());
        assert_eq!(c.value(), Some("custom"));

        // not offered: unset, not some other value
        let c = ChoiceInput::unsettable("inherit", CONTAINER_FOLDER_CHOICES, Some("none"));
        assert!(c.is_unset());
    }

    #[test]
    fn plain_choice_is_never_unset() {
        let c = ChoiceInput::new(FOLDER_CHOICES, "none"); // first option
        assert!(!c.is_unset());
        assert_eq!(c.value(), Some("none"));
    }

    fn choice(selected: usize) -> ChoiceInput {
        ChoiceInput {
            selected,
            ..ChoiceInput::new(FOLDER_CHOICES, "")
        }
    }

    #[test]
    fn handle_key_prev_correct() {
        let mut choice = choice(2);
        choice.handle_key(press(KeyCode::Left));
        assert_eq!(choice.selected, 1);
        choice.handle_key(press(KeyCode::Char('h')));
        assert_eq!(choice.selected, 0);
    }

    #[test]
    fn handle_key_next_correct() {
        let mut choice = choice(0);
        choice.handle_key(press(KeyCode::Right));
        assert_eq!(choice.selected, 1);
        choice.handle_key(press(KeyCode::Char('l')));
        assert_eq!(choice.selected, 2);
    }

    #[test]
    fn handle_key_next_incorrect() {
        let mut choice = choice(0);
        choice.handle_key(press(KeyCode::Char('i')));
        assert_eq!(choice.selected, 0);
    }

    #[test]
    fn handle_key_next_wraps() {
        let mut choice = choice(2);
        choice.handle_key(press(KeyCode::Right));
        assert_eq!(choice.selected, 0);
    }

    #[test]
    fn handle_key_prev_wraps() {
        let mut choice = choice(0);
        choice.handle_key(press(KeyCode::Left));
        assert_eq!(choice.selected, 2);
    }

    #[test]
    fn handle_key_reports_a_new_selection() {
        let mut choice = choice(0);
        assert!(choice.handle_key(press(KeyCode::Right)));
        assert!(choice.handle_key(press(KeyCode::Char('h'))));
        assert!(!choice.handle_key(press(KeyCode::Char('i')))); // not a choice key
    }

    #[test]
    fn a_single_option_never_changes() {
        let mut choice = ChoiceInput::new(&["only"], "only");
        assert!(!choice.handle_key(press(KeyCode::Right))); // wraps onto itself
        assert!(!choice.handle_key(press(KeyCode::Left)));
    }
}
