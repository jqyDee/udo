use chrono::TimeDelta;

use super::*;
use crate::{
    model::{
        node::Node,
        sessions::{Session, TaskRef},
        task::Task,
        time::{self, Minutes, Time},
        tree::Tree,
    },
    test_util::{at, container, session, task, tree_with},
};

/// A session on `task` (in `parent`), for tasks outside the tree too
/// (deleted ones).
fn on_node(task: &Node, parent: &Node, start: Time, end: Option<Time>) -> Session {
    Session {
        task: TaskRef::of(task, parent).unwrap(),
        ..session(start, end)
    }
}

/// A session on the task at `path` in `tree`.
fn on(tree: &Tree, path: &[usize], start: Time, end: Option<Time>) -> Session {
    let parent = &path[..path.len() - 1];
    on_node(
        tree.get(path).unwrap(),
        tree.get(parent).unwrap(),
        start,
        end,
    )
}

/// The id of the node at `path`.
fn id(tree: &Tree, path: &[usize]) -> NodeId {
    tree.get(path).unwrap().id()
}

/// A task marked done at `done`.
fn done_task(name: &str, done: Time) -> Node {
    let mut t = Task::new(None, time::now());
    t.done_at = Some(done);
    Node::task(name.into(), t)
}

/// `node` (a container) with its own `estimate` setting.
fn with_estimate(mut node: Node, minutes: u32) -> Node {
    node.as_container_mut().unwrap().settings.estimate = Some(Minutes::new(minutes));
    node
}

/// `node` created at `t` (fresh nodes get `now`, too close to tell apart).
fn created(mut node: Node, t: Time) -> Node {
    node.header.created_at = t;
    node
}

#[test]
fn build_sums_the_sessions_of_a_task() {
    // root > cs [0] > lab [0, 0]
    let tree = tree_with(vec![container("cs", vec![task("lab")])]);
    let sessions = [
        on(&tree, &[0, 0], at(9, 0), Some(at(10, 0))),
        on(&tree, &[0, 0], at(14, 0), Some(at(14, 30))),
    ];

    let history = History::build(&tree, &sessions, at(18, 0));

    let rec = history.task(id(&tree, &[0, 0])).unwrap();
    assert_eq!(rec.actual, Minutes::new(90));
    assert_eq!(rec.container, id(&tree, &[0]));
}

#[test]
fn build_counts_a_running_session_up_to_now() {
    let tree = tree_with(vec![container("cs", vec![task("lab")])]);
    let sessions = [on(&tree, &[0, 0], at(9, 0), None)];

    let history = History::build(&tree, &sessions, at(9, 30));

    let rec = history.task(id(&tree, &[0, 0])).unwrap();
    assert_eq!(rec.actual, Minutes::new(30));
}

#[test]
fn build_takes_done_and_created_from_the_tree() {
    let lab = created(done_task("lab", at(17, 0)), at(8, 0));
    let tree = tree_with(vec![container("cs", vec![lab, task("essay")])]);

    let history = History::build(&tree, &[], at(18, 0));

    let lab = history.task(id(&tree, &[0, 0])).unwrap();
    assert_eq!(lab.done_at, Some(at(17, 0)));
    assert_eq!(lab.created_at, at(8, 0));
    let essay = history.task(id(&tree, &[0, 1])).unwrap();
    assert_eq!(essay.done_at, None);
}

#[test]
fn build_keeps_tasks_without_sessions() {
    let tree = tree_with(vec![container("cs", vec![task("lab")])]);

    let history = History::build(&tree, &[], at(18, 0));

    let rec = history.task(id(&tree, &[0, 0])).unwrap();
    assert_eq!(rec.actual, Minutes::new(0));
    assert_eq!(history.tasks().len(), 1);
}

#[test]
fn build_keeps_sessions_of_deleted_tasks() {
    let tree = tree_with(vec![container("cs", vec![])]);
    let old = task("old"); // not in the tree: deleted
    let cs = tree.get(&[0]).unwrap();
    let sessions = [
        on_node(&old, cs, at(14, 0), Some(at(14, 30))),
        on_node(&old, cs, at(9, 0), Some(at(10, 0))),
    ];

    let history = History::build(&tree, &sessions, at(18, 0));

    let rec = history.task(old.id()).unwrap();
    assert_eq!(rec.container, id(&tree, &[0]));
    assert_eq!(rec.created_at, at(9, 0)); // the first session's start
    assert_eq!(rec.done_at, None);
    assert_eq!(rec.actual, Minutes::new(90));
}

#[test]
fn build_drops_deleted_tasks_of_unknown_containers() {
    let tree = tree_with(vec![container("cs", vec![])]);
    let gone = container("gone", vec![]); // deleted along with its task
    let old = task("old");
    let sessions = [on_node(&old, &gone, at(9, 0), Some(at(10, 0)))];

    let history = History::build(&tree, &sessions, at(18, 0));

    assert_eq!(history.task(old.id()), None);
    assert!(history.tasks().is_empty());
}

#[test]
fn build_ignores_deleted_sessions() {
    let tree = tree_with(vec![container("cs", vec![task("lab")])]);
    let sessions = [
        Session {
            deleted_at: Some(at(12, 0)),
            ..on(&tree, &[0, 0], at(9, 0), Some(at(10, 0)))
        },
        on(&tree, &[0, 0], at(14, 0), Some(at(14, 30))),
    ];

    let history = History::build(&tree, &sessions, at(18, 0));

    let rec = history.task(id(&tree, &[0, 0])).unwrap();
    assert_eq!(rec.actual, Minutes::new(30));
}

#[test]
fn tasks_are_sorted_oldest_first() {
    // tree order: late, early; plus a deleted task older than both
    let late = created(task("late"), at(11, 0));
    let early = created(task("early"), at(10, 0));
    let tree = tree_with(vec![container("cs", vec![late, early])]);
    let old = task("old");
    let sessions = [on_node(
        &old,
        tree.get(&[0]).unwrap(),
        at(9, 0),
        Some(at(9, 30)),
    )];

    let history = History::build(&tree, &sessions, at(18, 0));

    let order: Vec<NodeId> = history.tasks().iter().map(|t| t.id).collect();
    assert_eq!(
        order,
        vec![old.id(), id(&tree, &[0, 1]), id(&tree, &[0, 0])]
    );
}

#[test]
fn own_estimate_is_not_inherited() {
    let root = with_estimate(container("root", vec![container("cs", vec![])]), 60);
    let tree = Tree::new(root);

    let history = History::build(&tree, &[], at(18, 0));

    assert_eq!(history.own_estimate(tree.root.id()), Some(Minutes::new(60)));
    assert_eq!(history.own_estimate(id(&tree, &[0])), None);
}

#[test]
fn parent_links_follow_the_tree() {
    // root > uni [0] > cs [0, 0]
    let tree = tree_with(vec![container("uni", vec![container("cs", vec![])])]);

    let history = History::build(&tree, &[], at(18, 0));

    assert_eq!(history.parent(id(&tree, &[0, 0])), Some(id(&tree, &[0])));
    assert_eq!(history.parent(id(&tree, &[0])), Some(tree.root.id()));
    assert_eq!(history.parent(tree.root.id()), None);
    assert_eq!(history.parent(NodeId::new()), None); // unknown
}

#[test]
fn is_below_walks_up_the_parents() {
    // root > uni [0] > cs [0, 0]; root > home [1]
    let tree = tree_with(vec![
        container("uni", vec![container("cs", vec![])]),
        container("home", vec![]),
    ]);
    let history = History::build(&tree, &[], at(18, 0));
    let (uni, cs, home) = (id(&tree, &[0]), id(&tree, &[0, 0]), id(&tree, &[1]));

    assert!(history.is_below(cs, uni));
    assert!(history.is_below(cs, tree.root.id()));
    assert!(history.is_below(cs, cs)); // itself
    assert!(!history.is_below(uni, cs)); // not upward
    assert!(!history.is_below(cs, home)); // not sideways
}

// ---------- Average ----------

/// A finished session of `minutes` on the task at `path`.
fn worked(tree: &Tree, path: &[usize], minutes: i64) -> Session {
    let start = at(8, 0);
    on(tree, path, start, Some(start + TimeDelta::minutes(minutes)))
}

/// `Average` of the container at `path`, after `sessions`.
fn average_of(tree: &Tree, sessions: &[Session], path: &[usize]) -> Option<Estimate> {
    let history = History::build(tree, sessions, at(23, 0));
    Average.of_container(id(tree, path), &history)
}

/// A done task (the time comes from `worked`).
fn done(name: &str) -> Node {
    done_task(name, at(18, 0))
}

fn setting(container: NodeId, minutes: u32) -> Prior {
    Prior::Setting {
        container,
        minutes: Minutes::new(minutes),
    }
}

#[test]
fn no_prior_no_tasks_is_none() {
    let tree = tree_with(vec![container("cs", vec![])]);

    assert_eq!(average_of(&tree, &[], &[0]), None);
}

#[test]
fn setting_alone_is_the_prior() {
    let tree = tree_with(vec![with_estimate(container("cs", vec![]), 90)]);
    let cs = id(&tree, &[0]);

    assert_eq!(
        average_of(&tree, &[], &[0]),
        Some(Estimate {
            minutes: Minutes::new(90),
            basis: Basis::Prior(setting(cs, 90)),
        })
    );
}

#[test]
fn one_done_task_moves_the_prior() {
    let tree = tree_with(vec![with_estimate(container("cs", vec![done("a")]), 90)]);
    let cs = id(&tree, &[0]);
    let sessions = [worked(&tree, &[0, 0], 70)];

    // (3·90 + 70) / 4
    assert_eq!(
        average_of(&tree, &sessions, &[0]),
        Some(Estimate {
            minutes: Minutes::new(85),
            basis: Basis::Learned {
                container: cs,
                tasks: 1,
                prior: Some(setting(cs, 90)),
            },
        })
    );
}

#[test]
fn many_done_tasks_outweigh_the_prior() {
    let tasks = (0..10).map(|i| done(&format!("t{i}"))).collect();
    let tree = tree_with(vec![with_estimate(container("cs", tasks), 90)]);
    let sessions: Vec<Session> = (0..10).map(|i| worked(&tree, &[0, i], 60)).collect();

    let estimate = average_of(&tree, &sessions, &[0]).unwrap();

    // (3·90 + 10·60) / 13 = 66.9
    assert_eq!(estimate.minutes, Minutes::new(67));
    assert!(matches!(estimate.basis, Basis::Learned { tasks: 10, .. }));
}

#[test]
fn no_prior_is_the_plain_average() {
    let tree = tree_with(vec![container("cs", vec![done("a"), done("b")])]);
    let cs = id(&tree, &[0]);
    let sessions = [worked(&tree, &[0, 0], 60), worked(&tree, &[0, 1], 120)];

    assert_eq!(
        average_of(&tree, &sessions, &[0]),
        Some(Estimate {
            minutes: Minutes::new(90),
            basis: Basis::Learned {
                container: cs,
                tasks: 2,
                prior: None,
            },
        })
    );
}

#[test]
fn open_task_under_the_estimate_is_ignored() {
    // 30m so far says only "at least 30m": no reason to go below 85m
    let tree = tree_with(vec![with_estimate(
        container("cs", vec![done("a"), task("b")]),
        90,
    )]);
    let sessions = [worked(&tree, &[0, 0], 70), worked(&tree, &[0, 1], 30)];

    let estimate = average_of(&tree, &sessions, &[0]).unwrap();

    assert_eq!(estimate.minutes, Minutes::new(85));
    assert!(matches!(estimate.basis, Basis::Learned { tasks: 1, .. }));
}

#[test]
fn open_task_over_the_estimate_counts_half() {
    let tree = tree_with(vec![with_estimate(
        container("cs", vec![done("a"), task("b")]),
        90,
    )]);
    let sessions = [worked(&tree, &[0, 0], 70), worked(&tree, &[0, 1], 200)];

    let estimate = average_of(&tree, &sessions, &[0]).unwrap();

    // base (3·90 + 70) / 4 = 85; 200 > 85: (340 + ½·200) / 4.5 = 97.8
    assert_eq!(estimate.minutes, Minutes::new(98));
    assert!(matches!(estimate.basis, Basis::Learned { tasks: 2, .. }));
}

#[test]
fn open_tasks_without_a_base_do_not_count() {
    let tree = tree_with(vec![container("cs", vec![task("b")])]);
    let sessions = [worked(&tree, &[0, 0], 200)];

    assert_eq!(average_of(&tree, &sessions, &[0]), None);
}

#[test]
fn deleted_tasks_count_as_open() {
    let tree = tree_with(vec![with_estimate(container("cs", vec![done("a")]), 90)]);
    let old = task("old"); // deleted, 200m recorded
    let start = at(8, 0);
    let sessions = [
        worked(&tree, &[0, 0], 70),
        on_node(
            &old,
            tree.get(&[0]).unwrap(),
            start,
            Some(start + TimeDelta::minutes(200)),
        ),
    ];

    let estimate = average_of(&tree, &sessions, &[0]).unwrap();

    assert_eq!(estimate.minutes, Minutes::new(98)); // as an open task over 85
}

#[test]
fn zero_minute_tasks_do_not_count() {
    // done without the timer: no data
    let tree = tree_with(vec![with_estimate(container("cs", vec![done("a")]), 90)]);
    let cs = id(&tree, &[0]);

    assert_eq!(
        average_of(&tree, &[], &[0]),
        Some(Estimate {
            minutes: Minutes::new(90),
            basis: Basis::Prior(setting(cs, 90)),
        })
    );
}

#[test]
fn the_task_itself_does_not_count() {
    let tree = tree_with(vec![with_estimate(container("cs", vec![done("a")]), 90)]);
    let cs = id(&tree, &[0]);
    let sessions = [worked(&tree, &[0, 0], 600)];
    let history = History::build(&tree, &sessions, at(23, 0));

    assert_eq!(
        Average.estimate(id(&tree, &[0, 0]), &history),
        Some(Estimate {
            minutes: Minutes::new(90),
            basis: Basis::Prior(setting(cs, 90)),
        })
    );
}

#[test]
fn estimate_of_a_task_is_its_containers() {
    let tree = tree_with(vec![with_estimate(
        container("cs", vec![done("a"), task("b")]),
        90,
    )]);
    let sessions = [worked(&tree, &[0, 0], 70)];
    let history = History::build(&tree, &sessions, at(23, 0));

    let of_task = Average.estimate(id(&tree, &[0, 1]), &history);

    assert_eq!(of_task, Average.of_container(id(&tree, &[0]), &history));
    assert_eq!(of_task.unwrap().minutes, Minutes::new(85));
}

#[test]
fn unknown_task_has_no_estimate() {
    let tree = tree_with(vec![with_estimate(container("cs", vec![]), 90)]);
    let history = History::build(&tree, &[], at(23, 0));

    assert_eq!(Average.estimate(NodeId::new(), &history), None);
}

// ---------- the prior ----------

/// The prior of a learned estimate (fails on any other basis).
fn prior_of(estimate: Estimate) -> Option<Prior> {
    let Basis::Learned { prior, .. } = estimate.basis else {
        panic!("expected learned, got {:?}", estimate.basis);
    };
    prior
}

#[test]
fn own_setting_cuts_the_chain() {
    // root > uni [0] (3h) > cs [0, 0] (1h, done 600m)
    let cs = with_estimate(container("cs", vec![done("a")]), 60);
    let tree = tree_with(vec![with_estimate(container("uni", vec![cs]), 180)]);
    let cs = id(&tree, &[0, 0]);
    let sessions = [worked(&tree, &[0, 0, 0], 600)];

    let estimate = average_of(&tree, &sessions, &[0, 0]).unwrap();

    assert_eq!(prior_of(estimate), Some(setting(cs, 60)));
    // (3·60 + 600) / 4
    assert_eq!(estimate.minutes, Minutes::new(195));
}

#[test]
fn new_project_starts_from_its_siblings() {
    // root > uni [0] > cs [0, 0] (done 60m, 120m), physics [0, 1] (empty);
    // uni itself has no tasks, so its whole subtree is pooled
    let tree = tree_with(vec![container(
        "uni",
        vec![
            container("cs", vec![done("a"), done("b")]),
            container("physics", vec![]),
        ],
    )]);
    let uni = id(&tree, &[0]);
    let sessions = [
        worked(&tree, &[0, 0, 0], 60),
        worked(&tree, &[0, 0, 1], 120),
    ];

    assert_eq!(
        average_of(&tree, &sessions, &[0, 1]),
        Some(Estimate {
            minutes: Minutes::new(90),
            basis: Basis::Prior(Prior::Parent {
                container: uni,
                tasks: 2,
                minutes: Minutes::new(90),
            }),
        })
    );
}

#[test]
fn own_tasks_do_not_move_their_prior() {
    // root > uni [0] > cs [0, 0] (done 600m), math [0, 1] (done 60m)
    let tree = tree_with(vec![container(
        "uni",
        vec![
            container("cs", vec![done("a")]),
            container("math", vec![done("b")]),
        ],
    )]);
    let uni = id(&tree, &[0]);
    let sessions = [
        worked(&tree, &[0, 0, 0], 600),
        worked(&tree, &[0, 1, 0], 60),
    ];

    let estimate = average_of(&tree, &sessions, &[0, 0]).unwrap();

    // cs's prior: only math's 60m; with cs's 600m too it would be 330m
    assert_eq!(
        prior_of(estimate),
        Some(Prior::Parent {
            container: uni,
            tasks: 1,
            minutes: Minutes::new(60),
        })
    );
    // (3·60 + 600) / 4
    assert_eq!(estimate.minutes, Minutes::new(195));
}

#[test]
fn prior_goes_up_the_tree() {
    // root (1h30) > uni [0] > cs [0, 0], nothing done: passed on through uni
    let root = with_estimate(
        container(
            "root",
            vec![container("uni", vec![container("cs", vec![])])],
        ),
        90,
    );
    let tree = Tree::new(root);

    assert_eq!(
        average_of(&tree, &[], &[0, 0]),
        Some(Estimate {
            minutes: Minutes::new(90),
            basis: Basis::Prior(setting(tree.root.id(), 90)),
        })
    );
}

#[test]
fn parent_prior_blends_with_the_setting_above() {
    // root (1h30) > uni [0] > cs [0, 0] (done 70m), physics [0, 1] (empty)
    let root = with_estimate(
        container(
            "root",
            vec![container(
                "uni",
                vec![
                    container("cs", vec![done("a")]),
                    container("physics", vec![]),
                ],
            )],
        ),
        90,
    );
    let tree = Tree::new(root);
    let uni = id(&tree, &[0]);
    let sessions = [worked(&tree, &[0, 0, 0], 70)];

    // uni's pool (cs's 70m) blended with the root's 1h30: (3·90 + 70) / 4
    assert_eq!(
        average_of(&tree, &sessions, &[0, 1]),
        Some(Estimate {
            minutes: Minutes::new(85),
            basis: Basis::Prior(Prior::Parent {
                container: uni,
                tasks: 1,
                minutes: Minutes::new(85),
            }),
        })
    );
}

#[test]
fn of_node_on_a_task_is_its_estimate() {
    let tree = tree_with(vec![with_estimate(
        container("cs", vec![done("a"), task("b")]),
        90,
    )]);
    let sessions = [worked(&tree, &[0, 0], 70)];
    let history = History::build(&tree, &sessions, at(23, 0));
    let b = tree.get(&[0, 1]).unwrap();

    assert_eq!(of_node(b, &history), Average.estimate(b.id(), &history));
    assert_eq!(of_node(b, &history).unwrap().minutes, Minutes::new(85));
}

#[test]
fn of_node_on_a_container_is_its_average() {
    let tree = tree_with(vec![with_estimate(container("cs", vec![done("a")]), 90)]);
    let sessions = [worked(&tree, &[0, 0], 70)];
    let history = History::build(&tree, &sessions, at(23, 0));
    let cs = tree.get(&[0]).unwrap();

    // a counts here: it is not the node being estimated
    assert_eq!(
        of_node(cs, &history),
        Average.of_container(cs.id(), &history)
    );
    assert_eq!(of_node(cs, &history).unwrap().minutes, Minutes::new(85));
}

#[test]
fn nothing_set_anywhere_has_no_prior() {
    let tree = tree_with(vec![container("uni", vec![container("cs", vec![])])]);

    assert_eq!(average_of(&tree, &[], &[0, 0]), None);
}
