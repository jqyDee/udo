//! "Remove?" prompt: `d` opens it, y / n / esc answer it.

use crossterm::event::{KeyCode, KeyEvent};

use super::{App, Flow, Mode};
use crate::model::tree::NodePath;

/// What a pending "remove?" prompt is about. Stored when `d` is pressed, so
/// the answer always applies to the node that was selected at that moment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Confirm {
    pub path: NodePath,
    pub name: String,
}

impl App<'_> {
    /// Open the confirm prompt for the selected node. Nothing selected (the
    /// root, e.g. empty tree) -> error toast instead.
    pub(super) fn ask_delete(&mut self) {
        let path = self.tree.cursor.clone();
        match self.tree.get(&path) {
            Some(node) if !path.is_empty() => {
                let name = node.name().to_string();
                self.mode = Mode::Confirm(Confirm { path, name });
            }
            _ => self.error("nothing selected"),
        }
    }

    /// Answer to the confirm prompt: `y` removes the node (unregister only,
    /// files stay), `n`/esc cancel, anything else is ignored and the prompt
    /// stays open.
    pub(super) async fn answer_confirm(&mut self, key: KeyEvent) -> Flow {
        let yes = match key.code {
            KeyCode::Char('y') => true,
            KeyCode::Char('n') | KeyCode::Esc => false,
            _ => return Flow::Continue,
        };

        let Mode::Confirm(confirm) = std::mem::replace(&mut self.mode, Mode::Normal) else {
            return Flow::Continue;
        };
        if yes {
            match self.tree.delete(&confirm.path).await {
                Ok(()) => self.info(format!("removed {} (files kept)", confirm.name)),
                Err(e) => self.error(e.to_string()),
            }
        }

        Flow::Continue
    }
}
