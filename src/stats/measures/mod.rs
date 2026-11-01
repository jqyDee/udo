use crate::stats::interval::Piece;

pub mod minutes;
pub mod sessions;

/// What is counted per group: `minutes`, `sessions`.
pub trait Measure {
    fn name(&self) -> &'static str;
    fn new_acc(&self) -> Box<dyn Acc>;
}

/// The running value of one measure in one group.
pub trait Acc {
    fn add(&mut self, piece: &Piece<'_>);
    fn value(&self) -> serde_json::Value;
}
