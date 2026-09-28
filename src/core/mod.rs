//! One object for everything udo changes: the tree and the stores. The TUI
//! and the CLI read through it freely and write only through its methods,
//! so rules that span tree and sessions live in one place.

use std::path::Path;

use crate::{
    Res,
    model::tree::Tree,
    storage::{Storage, sessions::Sessions},
};

mod nodes;
mod settings;
mod status;
#[cfg(test)]
mod tests;
mod timer;

pub use nodes::TaskDefaults;

/// The tree and the stores, opened once at startup.
pub struct Core {
    tree: Tree, // private: every write goes through a `Core` method
    storage: Storage,
}

impl Core {
    /// Load the tree at `root` (created on the first run), then open
    /// `udo.db` inside it: the dir must exist before the database file.
    pub async fn open(root: &Path) -> Res<Self> {
        let tree = Tree::load_from(root).await?;
        let storage = Storage::open_db(root)?;
        Ok(Self::new(tree, storage))
    }

    /// A `Core` from parts (tests: a built tree + `Storage::in_memory()`).
    pub fn new(tree: Tree, storage: Storage) -> Self {
        Self { tree, storage }
    }

    /// The tree, to read.
    pub fn tree(&self) -> &Tree {
        &self.tree
    }

    /// The session store, to read (`running`, `query`).
    pub fn sessions(&self) -> &Sessions {
        &self.storage.sessions
    }
}
