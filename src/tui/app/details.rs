#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum DetailsTab {
    #[default]
    Info,
    Settings,
}

impl DetailsTab {
    pub const ALL: [Self; 2] = [Self::Info, Self::Settings];

    pub fn title(self) -> &'static str {
        match self {
            Self::Info => "details",
            Self::Settings => "settings",
        }
    }

    pub fn next(&mut self) {
        let count = Self::ALL.len();
        let idx = Self::ALL.iter().position(|p| p == self).unwrap();
        *self = Self::ALL[(idx + 1) % count];
    }

    pub fn prev(&mut self) {
        let count = Self::ALL.len();
        let idx = Self::ALL.iter().position(|p| p == self).unwrap();
        *self = Self::ALL[(idx + count - 1) % count];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn next_and_prev_walk_all_tabs_and_wrap() {
        let mut tab = DetailsTab::default();
        for expected in DetailsTab::ALL.iter().cycle().skip(1).take(5) {
            tab.next();
            assert_eq!(tab, *expected);
        }

        let mut tab = DetailsTab::default();
        for expected in DetailsTab::ALL.iter().rev().cycle().take(5) {
            tab.prev();
            assert_eq!(tab, *expected);
        }
    }

    #[test]
    fn titles_are_distinct() {
        let titles: std::collections::HashSet<_> =
            DetailsTab::ALL.iter().map(|t| t.title()).collect();
        assert_eq!(titles.len(), DetailsTab::ALL.len());
    }
}
