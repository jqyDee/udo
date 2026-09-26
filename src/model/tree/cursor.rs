use crate::model::tree::Tree;

impl Tree {
    /// Keep `self.cursor` valid after the child at `parent_path + [idx]` was removed.
    pub(super) fn fix_cursor_after_remove(&mut self, parent_path: &[usize], idx: usize) {
        let depth = parent_path.len();
        // Only cursors below the parent are affected.
        if self.cursor.len() <= depth || !self.cursor.starts_with(parent_path) {
            return;
        }

        let c = self.cursor[depth];
        if c > idx {
            // later sibling (or inside it): shifted left by one
            self.cursor[depth] -= 1;
        } else if c == idx {
            // on the removed node or inside it: take the sibling that moved
            // into the gap, else the previous one, else the parent
            let remaining = self.get(parent_path).map_or(0, |p| p.children().len());
            self.cursor.truncate(depth);
            if remaining > 0 {
                self.cursor.push(idx.min(remaining - 1));
            }
        }
        // c < idx: earlier sibling, unaffected
    }
}

#[cfg(test)]
mod tests {
    // ---------- cursor fix-up (in memory, no disk) ----------

    use crate::model::{NodePath, tree::tests::tree};

    /// tree() = root: [a, inner: [b]]. Remove `path` like `delete` does,
    /// without saving, and return the fixed-up cursor.
    fn cursor_after_remove(cursor: &[usize], path: &[usize]) -> NodePath {
        let mut t = tree();
        t.cursor = cursor.to_vec();
        let (&idx, parent) = path.split_last().unwrap();
        t.get_mut(parent)
            .unwrap()
            .children_mut()
            .unwrap()
            .remove(idx);
        t.fix_cursor_after_remove(parent, idx);
        t.cursor
    }

    #[test]
    fn cursor_on_later_sibling_shifts_left() {
        assert_eq!(cursor_after_remove(&[1], &[0]), vec![0]);
    }

    #[test]
    fn cursor_inside_later_sibling_shifts_left() {
        assert_eq!(cursor_after_remove(&[1, 0], &[0]), vec![0, 0]);
    }

    #[test]
    fn cursor_on_earlier_sibling_unchanged() {
        assert_eq!(cursor_after_remove(&[0], &[1]), vec![0]);
    }

    #[test]
    fn cursor_on_removed_takes_next_sibling() {
        assert_eq!(cursor_after_remove(&[0], &[0]), vec![0]); // now "inner"
    }

    #[test]
    fn cursor_on_removed_last_takes_previous_sibling() {
        assert_eq!(cursor_after_remove(&[1], &[1]), vec![0]); // now "a"
    }

    #[test]
    fn cursor_inside_removed_subtree_goes_to_sibling() {
        assert_eq!(cursor_after_remove(&[1, 0], &[1]), vec![0]);
    }

    #[test]
    fn cursor_on_removed_only_child_goes_to_parent() {
        assert_eq!(cursor_after_remove(&[1, 0], &[1, 0]), vec![1]);
    }

    #[test]
    fn cursor_on_root_unchanged() {
        assert_eq!(cursor_after_remove(&[], &[0]), Vec::<usize>::new());
    }
}
