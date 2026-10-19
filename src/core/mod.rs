//! One object for everything udo changes: the tree and the stores. The TUI
//! and the CLI read through it freely and write only through its methods,
//! so rules that span tree and sessions live in one place.

use std::{
    path::Path,
    sync::{
        Mutex,
        atomic::{AtomicU64, Ordering},
    },
};

use crate::{
    Res,
    model::tree::Tree,
    storage::{Storage, sessions::Sessions},
};

mod estimates;
mod nodes;
mod sessions;
mod settings;
mod status;
#[cfg(test)]
mod tests;
mod timer;

pub use nodes::TaskDefaults;
pub use sessions::SPLIT_AT_EDGE;
pub use timer::IsDone;

/// The tree and the stores, opened once at startup.
pub struct Core {
    tree: Tree, // private: every write goes through a `Core` method
    storage: Storage,
    /// What went wrong after an action succeeded (`estimate not recorded:
    /// ...`): the action stands, the CLI / TUI show these after each call.
    /// A `Mutex`: `&self` methods (`add_session`) warn too.
    warnings: Mutex<Vec<String>>,
    /// How many writes so far (`changes`): the TUI rebuilds its history
    /// only when this moved (#61). Bumped before writing, so a failed write
    /// counts too (it may have changed part of the data). Atomic: `&self`
    /// methods (`add_session`) write too.
    changes: AtomicU64,
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
        Self {
            tree,
            storage,
            warnings: Mutex::default(),
            changes: AtomicU64::default(),
        }
    }

    /// How many writes so far; two calls differ when something was written
    /// in between (through this `Core`: changes from the CLI elsewhere are
    /// not counted).
    pub fn changes(&self) -> u64 {
        self.changes.load(Ordering::Relaxed)
    }

    /// Count a write; first thing in every write method.
    fn changed(&self) {
        self.changes.fetch_add(1, Ordering::Relaxed);
    }

    /// The tree, to read.
    pub fn tree(&self) -> &Tree {
        &self.tree
    }

    /// The session store, to read (`running`, `query`).
    pub fn sessions(&self) -> &Sessions {
        &self.storage.sessions
    }

    /// Note a warning for the CLI / TUI (the action itself succeeded).
    fn warn(&self, warning: impl Into<String>) {
        self.warnings.lock().unwrap().push(warning.into());
    }

    /// The warnings since the last call, oldest first; afterwards none are
    /// left. The CLI / TUI call this after every action.
    pub fn take_warnings(&self) -> Vec<String> {
        std::mem::take(&mut self.warnings.lock().unwrap())
    }
}
