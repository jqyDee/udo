//! Container settings.

use crate::{
    Res,
    core::Core,
    model::settings::{ContainerSettings, RootSettings},
};

impl Core {
    /// Replace the own settings of the container at `path` and save them.
    /// `root`: the `[root]` settings, only for the root. Errors like
    /// `Tree::set_settings` (a task, a missing path, `root` elsewhere).
    pub async fn set_settings(
        &mut self,
        path: &[usize],
        settings: ContainerSettings,
        root: Option<RootSettings>,
    ) -> Res<()> {
        self.tree.set_settings(path, settings, root).await
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        core::Core,
        model::{node::Node, settings::ContainerSettings, tree::Tree},
        storage::Storage,
        test_util::disk_tree,
    };

    #[tokio::test]
    async fn settings_are_replaced_and_saved() {
        let (tmp, tree) = disk_tree().await;
        let mut core = Core::new(tree, Storage::in_memory());
        let settings = ContainerSettings {
            estimate: Some("1h30".parse().unwrap()),
            ..Default::default()
        };

        core.set_settings(&[1], settings.clone(), None)
            .await
            .unwrap();

        let reloaded = Tree::load_from(tmp.path()).await.unwrap();
        let ws = reloaded.get(&[1]).and_then(Node::as_container).unwrap();
        assert_eq!(ws.settings, settings);
    }

    #[tokio::test]
    async fn tasks_have_no_settings() {
        let (_tmp, tree) = disk_tree().await;
        let mut core = Core::new(tree, Storage::in_memory());

        let result = core
            .set_settings(&[0], ContainerSettings::default(), None)
            .await;

        assert!(result.is_err());
    }
}
