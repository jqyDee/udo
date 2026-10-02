//! State of the sessions tab's list: which page it shows, how long a page
//! is and which session the list cursor is on. No drawing, no store: the
//! rows come from `App.sessions`.

use std::ops::Range;

use crate::model::{id::NodeId, sessions::SessionId};

/// Which part of the cursor node's sessions the sessions tab shows.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct SessionList {
    /// Node whose sessions these are; another node at the tree cursor:
    /// start over (`follow`).
    pub node: Option<NodeId>,
    /// 0-based. Follows the selection (`select`); `h` / `l` turn it.
    pub page: usize,
    /// Rows per page, set while drawing (`view::details::page_len`, from
    /// the pane height). 0: not drawn yet.
    pub page_len: usize,
    /// The session under the list cursor. In `Mode::Sessions`, `None`
    /// means the list is empty; outside it, the cursor is in the tree. An
    /// id, not a row: it stays on its session when rows move.
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

    /// Row of the selected session in `ids` (newest first); None: nothing
    /// selected, or it is not there (anymore).
    pub fn index(&self, ids: &[SessionId]) -> Option<usize> {
        self.selected
            .and_then(|id| ids.iter().position(|&i| i == id))
    }

    /// Select the session at row `i` of `ids` (newest first) and show its
    /// page: the selection is always on the shown page.
    pub fn select(&mut self, ids: &[SessionId], i: usize) {
        self.selected = ids.get(i).copied();
        self.page = i / self.page_len.max(1);
    }

    /// A new page length from drawing (resize, a form opened or closed).
    /// The page follows the selection, so it stays on screen; nothing
    /// selected: the page stays.
    pub fn set_page_len(&mut self, page_len: usize, ids: &[SessionId]) {
        self.page_len = page_len;
        if let Some(i) = self.index(ids) {
            self.select(ids, i);
        }
    }

    /// `j` / `k`: `delta` rows down (older) or up (newer); stops at the ends.
    pub fn move_by(&mut self, ids: &[SessionId], delta: isize) {
        let Some(i) = self.index(ids) else { return };
        let target = i
            .saturating_add_signed(delta)
            .min(ids.len().saturating_sub(1));
        self.select(ids, target);
    }

    /// After a reload: `old` / `new` are the ids before and after (newest
    /// first), `node` the node at the tree cursor.
    ///
    /// - another node: page 1, nothing selected
    /// - the selected session still there: the cursor stays on it, its page
    ///   follows (new sessions on top push it down)
    /// - gone (removed elsewhere): the cursor on the row where it was, or
    ///   the last row
    /// - gone, and the list is empty now: nothing selected, page 1
    /// - nothing selected: only the page is kept inside the list
    pub fn follow(&mut self, node: NodeId, old: &[SessionId], new: &[SessionId]) {
        if self.node != Some(node) {
            *self = Self {
                node: Some(node),
                page_len: self.page_len, // from drawing, not from the node
                ..Default::default()
            };
            return;
        }
        match (self.selected, self.index(new)) {
            (None, _) => self.page = self.page.min(self.pages(new.len()) - 1),
            (Some(_), Some(i)) => self.select(new, i),
            (Some(_), None) if new.is_empty() => {
                self.selected = None;
                self.page = 0;
            }
            (Some(_), None) => {
                let was = self.index(old).unwrap_or(0);
                self.select(new, was.min(new.len() - 1));
            }
        }
    }

    /// `h` / `l`: `delta` pages back / on, the cursor on the page's first
    /// row; stops at the first / last page.
    pub fn turn_page(&mut self, ids: &[SessionId], delta: isize) {
        let last = self.pages(ids.len()) - 1;
        let page = self.page.saturating_add_signed(delta).min(last);
        self.select(ids, page * self.page_len.max(1));
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

    // ---------- moving (7 ids, 3 per page: rows 0-2, 3-5, 6) ----------

    fn ids() -> Vec<SessionId> {
        (0..7).map(|_| SessionId::new()).collect()
    }

    /// `ids` with 3 rows per page, row `row` selected.
    fn at_row(ids: &[SessionId], row: usize) -> SessionList {
        let mut l = list(0, 3);
        l.select(ids, row);
        l
    }

    #[test]
    fn select_shows_the_page_of_the_row() {
        let ids = ids();

        let l = at_row(&ids, 4);

        assert_eq!((l.index(&ids), l.page), (Some(4), 1));
        assert_eq!(l.selected, Some(ids[4]));
    }

    #[test]
    fn move_down_and_up() {
        let ids = ids();
        let mut l = at_row(&ids, 0);

        l.move_by(&ids, 1);
        assert_eq!(l.index(&ids), Some(1));
        l.move_by(&ids, -1);
        assert_eq!(l.index(&ids), Some(0));
    }

    #[test]
    fn moving_stops_at_the_ends() {
        let ids = ids();
        let mut first = at_row(&ids, 0);
        let mut last = at_row(&ids, 6);

        first.move_by(&ids, -1);
        last.move_by(&ids, 1);

        assert_eq!(first.index(&ids), Some(0));
        assert_eq!((last.index(&ids), last.page), (Some(6), 2));
    }

    #[test]
    fn moving_past_the_page_end_turns_the_page() {
        let ids = ids();
        let mut l = at_row(&ids, 2); // last row of page 0

        l.move_by(&ids, 1);
        assert_eq!((l.index(&ids), l.page), (Some(3), 1));
        l.move_by(&ids, -1);
        assert_eq!((l.index(&ids), l.page), (Some(2), 0));
    }

    #[test]
    fn turn_page_selects_the_first_row() {
        let ids = ids();
        let mut l = at_row(&ids, 1);

        l.turn_page(&ids, 1);
        assert_eq!((l.index(&ids), l.page), (Some(3), 1));
        l.turn_page(&ids, 1);
        assert_eq!((l.index(&ids), l.page), (Some(6), 2)); // the short last page
        l.turn_page(&ids, -1);
        assert_eq!((l.index(&ids), l.page), (Some(3), 1));
    }

    #[test]
    fn turning_stops_at_the_first_and_last_page() {
        let ids = ids();
        let mut last = at_row(&ids, 6);
        let mut first = at_row(&ids, 2);

        last.turn_page(&ids, 1);
        first.turn_page(&ids, -1);

        assert_eq!((last.index(&ids), last.page), (Some(6), 2));
        assert_eq!((first.index(&ids), first.page), (Some(0), 0)); // its first row
    }

    #[test]
    fn nothing_selected_does_not_move() {
        let ids = ids();
        let mut l = list(0, 3);

        l.move_by(&ids, 1);

        assert_eq!(l, list(0, 3));
    }

    /// Selected, but gone from the list (removed elsewhere): no move.
    #[test]
    fn a_selection_not_in_the_list_does_not_move() {
        let ids = ids();
        let mut l = list(1, 3);
        l.selected = Some(SessionId::new());

        l.move_by(&ids, 1);

        assert_eq!(l.index(&ids), None);
        assert_eq!(l.page, 1);
    }

    /// Shorter pages (a form opened): the page follows the selection.
    #[test]
    fn a_new_page_len_keeps_the_selection_on_screen() {
        let ids = ids();
        let mut l = at_row(&ids, 4); // 3 per page: page 1

        l.set_page_len(2, &ids);
        assert_eq!((l.page_len, l.page), (2, 2)); // rows 4-5
        l.set_page_len(6, &ids);
        assert_eq!(l.page, 0); // rows 0-5
        assert_eq!(l.index(&ids), Some(4));
    }

    #[test]
    fn a_new_page_len_without_selection_keeps_the_page() {
        let ids = ids();
        let mut l = list(1, 3);

        l.set_page_len(2, &ids);

        assert_eq!((l.page_len, l.page), (2, 1));
    }

    // ---------- follow (after a reload) ----------

    /// `at_row`, belonging to `node`.
    fn on_node(node: NodeId, ids: &[SessionId], row: usize) -> SessionList {
        SessionList {
            node: Some(node),
            ..at_row(ids, row)
        }
    }

    #[test]
    fn the_first_follow_sets_the_node() {
        let (node, ids) = (NodeId::new(), ids());
        let mut l = list(2, 3);

        l.follow(node, &[], &ids);

        assert_eq!(l.node, Some(node));
        assert_eq!((l.page, l.page_len, l.selected), (0, 3, None));
    }

    #[test]
    fn a_new_session_on_top_keeps_the_cursor_on_its_session() {
        let (node, old) = (NodeId::new(), ids());
        let mut l = on_node(node, &old, 2); // last row of page 0
        let mut new = vec![SessionId::new()];
        new.extend(&old);

        l.follow(node, &old, &new);

        assert_eq!(l.selected, Some(old[2]));
        assert_eq!((l.index(&new), l.page), (Some(3), 1)); // pushed onto page 1
    }

    #[test]
    fn a_removed_session_leaves_the_cursor_on_its_row() {
        let (node, old) = (NodeId::new(), ids());
        let mut l = on_node(node, &old, 4);
        let new: Vec<_> = old.iter().copied().filter(|&id| id != old[4]).collect();

        l.follow(node, &old, &new);

        assert_eq!(l.selected, Some(old[5])); // the next older one, now row 4
        assert_eq!(l.index(&new), Some(4));
    }

    #[test]
    fn the_last_row_removed_goes_to_the_new_last_row() {
        let (node, old) = (NodeId::new(), ids());
        let mut l = on_node(node, &old, 6);
        let new = &old[..6];

        l.follow(node, &old, new);

        assert_eq!(l.selected, Some(old[5]));
        assert_eq!(l.page, 1);
    }

    #[test]
    fn all_removed_selects_nothing() {
        let (node, old) = (NodeId::new(), ids());
        let mut l = on_node(node, &old, 4);

        l.follow(node, &old, &[]);

        assert_eq!((l.selected, l.page), (None, 0));
    }

    #[test]
    fn another_node_starts_over() {
        let (node, ids) = (NodeId::new(), ids());
        let mut l = on_node(node, &ids, 6); // page 2

        l.follow(NodeId::new(), &ids, &ids);

        assert_eq!((l.page, l.page_len, l.selected), (0, 3, None));
    }

    #[test]
    fn a_shrunk_list_clamps_the_page() {
        let (node, old) = (NodeId::new(), ids());
        let mut l = SessionList {
            node: Some(node),
            ..list(2, 3)
        };

        l.follow(node, &old, &old[..2]); // one page left

        assert_eq!(l.page, 0);
    }

    /// Nothing changed (a tick): nothing moves.
    #[test]
    fn the_same_list_changes_nothing() {
        let (node, ids) = (NodeId::new(), ids());
        let mut l = on_node(node, &ids, 4);
        let before = l;

        l.follow(node, &ids, &ids);

        assert_eq!(l, before);
    }
}
