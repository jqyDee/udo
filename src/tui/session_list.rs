//! State of the sessions tab's list: which page it shows, how long a page
//! is and which session the list cursor is on. No drawing, no store: the
//! rows come from `App.sessions`.

use std::ops::Range;

use crate::model::sessions::SessionId;

/// Which part of the cursor node's sessions the sessions tab shows.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct SessionList {
    /// 0-based; stays 0 until stage 3 (`h` / `l`).
    pub page: usize,
    /// Rows per page, set while drawing (`view::details::page_len`, from
    /// the pane height). 0: not drawn yet.
    pub page_len: usize,
    /// The session under the list cursor (`Mode::Sessions`); `None`: the
    /// cursor is in the tree. An id, not a row: it stays on its session
    /// when rows move.
    pub selected: Option<SessionId>,
}

impl SessionList {
    /// Number of pages for `total` sessions (at least 1, also when empty).
    pub fn pages(&self, total: usize) -> usize {
        total.div_ceil(self.page_len.max(1)).max(1)
    }

    /// Indices into the newest-first list that the current page shows. A
    /// `page` past the end (the list shrank): the last page.
    pub fn range(&self, total: usize) -> Range<usize> {
        let len = self.page_len.max(1);
        let page = self.page.min(self.pages(total) - 1);
        let start = page * len;
        start..(start + len).min(total)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Paging only: the selection does not matter here.
    fn list(page: usize, page_len: usize) -> SessionList {
        SessionList {
            page,
            page_len,
            ..Default::default()
        }
    }

    #[test]
    fn empty_is_one_empty_page() {
        assert_eq!(list(0, 3).pages(0), 1);
        assert_eq!(list(0, 3).range(0), 0..0);
    }

    #[test]
    fn pages_split_by_page_len() {
        assert_eq!(list(0, 3).pages(7), 3);
        assert_eq!(list(0, 3).range(7), 0..3);
        assert_eq!(list(1, 3).range(7), 3..6);
        assert_eq!(list(2, 3).range(7), 6..7); // last page: the rest
        assert_eq!(list(0, 3).pages(6), 2); // exactly full: no extra page
    }

    #[test]
    fn a_page_past_the_end_shows_the_last_page() {
        assert_eq!(list(5, 3).range(7), 6..7);
        assert_eq!(list(5, 3).range(0), 0..0);
    }

    /// Before the first draw (`page_len` 0): one row per page, no panic.
    #[test]
    fn page_len_zero_counts_as_one() {
        assert_eq!(list(0, 0).pages(2), 2);
        assert_eq!(list(1, 0).range(2), 1..2);
    }
}
