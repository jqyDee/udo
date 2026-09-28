//! Task status (to do, in progress, done, …).

use crate::{Res, core::Core, model::task::TaskStatus};

impl Core {
    /// Set the status of the task at `path` and save it. Not a task: error.
    pub async fn set_status(&mut self, path: &[usize], status: TaskStatus) -> Res<()> {
        self.tree.set_task_status(path, status).await
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        core::Core,
        model::{node::Node, task::TaskStatus, tree::Tree},
        storage::Storage,
        test_util::disk_tree,
    };

    #[tokio::test]
    async fn the_status_is_set_and_saved() {
        let (tmp, tree) = disk_tree().await;
        let mut core = Core::new(tree, Storage::in_memory());

        core.set_status(&[0], TaskStatus::Finished).await.unwrap();

        let reloaded = Tree::load_from(tmp.path()).await.unwrap();
        let a = reloaded.get(&[0]).and_then(Node::as_task).unwrap();
        assert_eq!(a.status, TaskStatus::Finished);
    }

    #[tokio::test]
    async fn containers_have_no_status() {
        let (_tmp, tree) = disk_tree().await;
        let mut core = Core::new(tree, Storage::in_memory());

        assert!(core.set_status(&[1], TaskStatus::Finished).await.is_err());
    }
}
