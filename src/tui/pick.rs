//! Generic picker, the same shape as `Form` + `FormAction`: the picker
//! knows only its keys and its cursor, `PickAction` says what the pick
//! leads to. `o` / `O` (in `open`) open it.

use crate::model::{NodePath, settings::RunName};

/// A list to pick one item from (`Mode::Pick`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Picker {
    /// Box title, filled once when opening: `open lab 3 with`.
    pub title: String,
    /// Never empty: no items, no picker (a toast instead).
    pub items: Vec<PickItem>,
    /// Index into `items`.
    pub cursor: usize,
    pub action: PickAction,
}

/// One line of the picker and what picking it means.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PickItem {
    pub label: String,
    /// Dim after the label: `(default)`.
    pub note: Option<&'static str>,
    pub value: PickValue,
}

/// What an item stands for. Kept in the item itself, so label and value
/// cannot drift apart (no second list indexed by the cursor).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PickValue {
    Script(RunName),
    Task(NodePath),
}

/// What Enter does with the picked value. Stored when the picker opens, so
/// the pick applies to what was selected at that moment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PickAction {
    /// `O`: which script opens `path`; then the task picker (container)
    /// or `Flow::Run`.
    Script { path: NodePath },
    /// `o` / `O` on a container: which task below it the time goes to.
    Task {
        container: NodePath,
        script: RunName,
    },
}

impl Picker {
    /// Cursor one item down; stays on the last (no wrap, like the
    /// sessions list).
    pub fn down(&mut self) {
        self.cursor = (self.cursor + 1).min(self.items.len() - 1);
    }

    /// Cursor one item up; stays on the first.
    pub fn up(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }

    /// The value under the cursor.
    pub fn picked(&self) -> &PickValue {
        &self.items[self.cursor].value
    }
}
