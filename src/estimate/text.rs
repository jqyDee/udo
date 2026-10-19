//! Texts shared by the TUI details and `udo show` (#9): the same words in
//! both. Plain strings; `None` means "no row". Colour is the TUI's.

use crate::{
    estimate::{Basis, Prior, Shown, Totals},
    model::{
        time::{Left, Minutes},
        tree::Tree,
    },
};

/// A container's `estimated`: `12h30 (9 of 11 tasks)`, `- (0 of 11 tasks)`.
pub fn estimated_text(t: &Totals) -> String {
    let sum = if t.estimated_tasks == 0 {
        "-".into()
    } else {
        t.estimated.to_string()
    };
    format!("{sum} ({} of {} tasks)", t.estimated_tasks, t.tasks)
}

/// A container's `left`: `4h20`, `4h20 (2 over)`. `None` (no row) without
/// an estimated task.
pub fn totals_left_text(t: &Totals) -> Option<String> {
    (t.estimated_tasks > 0).then(|| match t.over {
        0 => t.left.to_string(),
        n => format!("{} ({n} over)", t.left),
    })
}

/// A container's `done`: `7h estimated, 8h10 tracked (over by 1h10)`.
/// `None` (no row) without an estimated done task.
pub fn done_text(t: &Totals) -> Option<String> {
    (t.done_estimated.get() > 0).then(|| {
        format!(
            "{} estimated, {} tracked ({})",
            t.done_estimated,
            t.done_actual,
            result_text(t.done_estimated, t.done_actual),
        )
    })
}

/// Why the estimate is what it is, for the brackets after it: `learned
/// from 6 done tasks`, `learned from 1 open task`, `learned from 5 done,
/// 1 open` (both: short, it has to fit the details row), `from uni` (its
/// setting), `from uni's tasks` (pooled), `yours` (a task's own).
/// A container no longer in the tree: `?` (never a panic in a draw).
pub fn basis_text(tree: &Tree, basis: &Basis) -> String {
    let name = |id| {
        tree.path_of(id)
            .and_then(|p| tree.get(&p).map(|n| n.name().to_string()))
            .unwrap_or_else(|| "?".into())
    };
    match basis {
        Basis::Own => "yours".into(),
        Basis::Learned {
            done_tasks: d,
            open_tasks: 0,
            ..
        } => format!("learned from {}", count(*d, "done")),
        Basis::Learned {
            done_tasks: 0,
            open_tasks: o,
            ..
        } => format!("learned from {}", count(*o, "open")),
        Basis::Learned {
            done_tasks: d,
            open_tasks: o,
            ..
        } => format!("learned from {d} done, {o} open"), // short: the row is narrow
        Basis::Prior(Prior::Setting { container, .. }) => format!("from {}", name(*container)),
        Basis::Prior(Prior::Parent { container, .. }) => {
            format!("from {}'s tasks", name(*container))
        }
    }
}

/// The estimate row: yours with udo's next to it (`1h30 (yours, udo: 1h20)`),
/// else udo's with why (`1h20 (learned from 3 done tasks)`), else `-`.
pub fn estimate_text(tree: &Tree, shown: &Shown) -> String {
    match (shown.own, &shown.learned) {
        (Some(own), Some(l)) => format!("{own} (yours, udo: {})", l.minutes),
        (Some(own), None) => format!("{own} (yours)"),
        (None, Some(e)) => format!("{} ({})", e.minutes, basis_text(tree, &e.basis)),
        (None, None) => "-".into(),
    }
}

/// An open task's `left`: `1h10`, `over by 20m`.
pub fn left_text(left: Left) -> String {
    match left {
        Left::Left(m) => m.to_string(),
        Left::Over(m) => format!("over by {m}"),
    }
}

/// A done task's `result`: `over by 20m`, `under by 10m`, `as estimated`.
pub fn result_text(estimate: Minutes, actual: Minutes) -> String {
    match Left::of(estimate, actual) {
        Left::Left(m) if m.get() == 0 => "as estimated".into(),
        Left::Left(m) => format!("under by {m}"),
        Left::Over(m) => format!("over by {m}"),
    }
}

/// `1 done task`, `2 open tasks` (for `basis_text`).
fn count(n: usize, what: &str) -> String {
    if n == 1 {
        format!("1 {what} task")
    } else {
        format!("{n} {what} tasks")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        estimate::Estimate,
        test_util::{container, tree_with},
    };

    fn m(minutes: u32) -> Minutes {
        Minutes::new(minutes)
    }

    #[test]
    fn result_over_under_or_as_estimated() {
        assert_eq!(result_text(m(60), m(80)), "over by 20m");
        assert_eq!(result_text(m(60), m(50)), "under by 10m");
        assert_eq!(result_text(m(60), m(60)), "as estimated");
    }

    #[test]
    fn left_or_over() {
        assert_eq!(left_text(Left::of(m(90), m(20))), "1h10");
        assert_eq!(left_text(Left::of(m(60), m(80))), "over by 20m");
    }

    #[test]
    fn container_rows() {
        let t = Totals {
            tasks: 11,
            estimated_tasks: 9,
            estimated: m(750),
            left: m(260),
            over: 2,
            done_estimated: m(420),
            done_actual: m(490),
            ..Totals::default()
        };
        assert_eq!(estimated_text(&t), "12h30 (9 of 11 tasks)");
        assert_eq!(totals_left_text(&t).as_deref(), Some("4h20 (2 over)"));
        assert_eq!(
            done_text(&t).as_deref(),
            Some("7h estimated, 8h10 tracked (over by 1h10)")
        );
    }

    #[test]
    fn container_without_estimates_has_no_left_or_done_row() {
        let t = Totals {
            tasks: 11,
            ..Totals::default()
        };
        assert_eq!(estimated_text(&t), "- (0 of 11 tasks)");
        assert_eq!(totals_left_text(&t), None);
        assert_eq!(done_text(&t), None);
    }

    #[test]
    fn container_left_without_over_has_no_brackets() {
        let t = Totals {
            tasks: 2,
            estimated_tasks: 2,
            left: m(260),
            ..Totals::default()
        };
        assert_eq!(totals_left_text(&t).as_deref(), Some("4h20"));
    }

    #[test]
    fn estimate_yours_learned_or_none() {
        let tree = tree_with(vec![]);
        let learned = Estimate {
            minutes: m(80),
            basis: Basis::Learned {
                container: tree.root.id(),
                done_tasks: 3,
                open_tasks: 0,
                prior: None,
            },
            as_of: None,
        };
        let shown = |own, learned| Shown { own, learned };

        assert_eq!(
            estimate_text(&tree, &shown(Some(m(90)), Some(learned))),
            "1h30 (yours, udo: 1h20)"
        );
        assert_eq!(
            estimate_text(&tree, &shown(Some(m(90)), None)),
            "1h30 (yours)"
        );
        assert_eq!(
            estimate_text(&tree, &shown(None, Some(learned))),
            "1h20 (learned from 3 done tasks)"
        );
        assert_eq!(estimate_text(&tree, &shown(None, None)), "-");
    }

    #[test]
    fn basis_names_the_setting_and_never_panics_on_a_gone_container() {
        let tree = tree_with(vec![container("uni", vec![])]);
        let uni = tree.get(&[0]).unwrap().id();
        let gone = container("gone", vec![]).id();
        let setting = |container| {
            Basis::Prior(Prior::Setting {
                container,
                minutes: m(60),
            })
        };

        assert_eq!(basis_text(&tree, &setting(uni)), "from uni");
        assert_eq!(basis_text(&tree, &setting(gone)), "from ?");
        assert_eq!(basis_text(&tree, &Basis::Own), "yours");
    }
}
