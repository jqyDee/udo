use crate::{core::IsDone, model::time, tui::app::App};

impl App<'_> {
    pub(super) async fn toggle_timer(&mut self) {
        let path = self.tree_state.cursor.clone();
        let Some(node) = self
            .core
            .tree()
            .get(&path)
            .filter(|n| n.as_task().is_some())
        else {
            return self.error("only tasks can be timed");
        };
        let (id, name) = (node.id(), node.name().to_string());
        let now = time::now();
        let is_this_on = self.running.as_ref().is_some_and(|s| s.task.id == id);
        let result = if is_this_on {
            self.core.stop(now).await.map(|s| match s {
                Some(s) => format!("stopped {name} ({})", s.duration(now)),
                None => format!("{name}: no timer running"),
            })
        } else {
            self.core
                .start(&path, now)
                .await
                .map(|_| format!("▶ {name}"))
        };
        match result {
            Ok(msg) => self.info(msg),
            Err(e) if e.downcast_ref::<IsDone>().is_some() => {
                self.error(format!("{e}: press x to reopen it"))
            }
            Err(e) => self.error(e.to_string()),
        }
    }
}
