//! Which sessions a scope covers: the rule `session list` and `stats` share.

use std::collections::HashMap;

use crate::model::{id::NodeId, sessions::Session, tree::Tree};

/// Whether a session is in the scope `scope` (a node's path; empty: the
/// root, everything). A live task's session is in when its task is below
/// the scope. A removed task (not in the tree) is in only with
/// `removed_tasks`, and then when its container is below the scope; from
/// the root, all of them (their container may be gone too).
pub fn in_scope(tree: &Tree, scope: &[usize], removed_tasks: bool) -> impl Fn(&Session) -> bool {
    let mut live: HashMap<NodeId, bool> = HashMap::new(); // task id -> below the scope
    let mut containers: Vec<NodeId> = vec![];
    for r in tree.rows() {
        let below = r.path.starts_with(scope);
        if r.node.as_task().is_some() {
            live.insert(r.node.id(), below);
        } else if below {
            containers.push(r.node.id());
        }
    }
    let root = scope.is_empty();
    move |s| match live.get(&s.task.id) {
        Some(below) => *below,
        None => removed_tasks && (root || containers.contains(&s.task.container_id)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::{at, session, uni_tree};

    /// A session on a task with id `task`, in the container `container`.
    fn of(task: NodeId, container: NodeId) -> Session {
        let mut s = session(at(9, 0), Some(at(10, 0)));
        s.task.id = task;
        s.task.container_id = container;
        s
    }

    #[test]
    fn a_live_task_is_in_below_the_scope_only() {
        let tree = uni_tree();
        let lab = tree.get(&[0, 0, 0]).unwrap().id();
        let s = of(lab, NodeId::new());

        assert!(in_scope(&tree, &[0], false)(&s)); // uni
        assert!(in_scope(&tree, &[0, 0], false)(&s)); // cs
        assert!(!in_scope(&tree, &[1], false)(&s)); // work
    }

    #[test]
    fn from_the_root_a_live_task_is_in() {
        let tree = uni_tree();
        let lab = tree.get(&[0, 0, 0]).unwrap().id();
        let s = of(lab, NodeId::new());

        assert!(in_scope(&tree, &[], false)(&s));
    }

    #[test]
    fn a_removed_task_needs_the_flag() {
        let tree = uni_tree();
        let cs = tree.get(&[0, 0]).unwrap().id();
        let s = of(NodeId::new(), cs); // not in the tree

        assert!(!in_scope(&tree, &[0], false)(&s));
        assert!(in_scope(&tree, &[0], true)(&s));
    }

    #[test]
    fn a_removed_task_is_in_when_its_container_is_below_the_scope() {
        let tree = uni_tree();
        let work = tree.get(&[1]).unwrap().id();
        let s = of(NodeId::new(), work);

        assert!(!in_scope(&tree, &[0], true)(&s));
        assert!(in_scope(&tree, &[1], true)(&s));
    }

    #[test]
    fn from_the_root_every_removed_task_is_in() {
        let tree = uni_tree();
        let gone = of(NodeId::new(), NodeId::new()); // its container may be gone too

        assert!(in_scope(&tree, &[], true)(&gone));
    }
}
