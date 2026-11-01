use std::collections::HashSet;

use crate::{
    model::sessions::SessionId,
    stats::{
        interval::Piece,
        measures::{Acc, Measure},
    },
};

/// Distinct sessions touching the group. A session cut in two groups counts
/// in both; the total counts it once.
pub struct SessionsMeasure;

impl Measure for SessionsMeasure {
    fn name(&self) -> &'static str {
        "sessions"
    }

    fn new_acc(&self) -> Box<dyn Acc> {
        Box::new(SessionsAcc {
            ids: HashSet::new(),
        })
    }
}

struct SessionsAcc {
    ids: HashSet<SessionId>,
}

impl Acc for SessionsAcc {
    fn add(&mut self, piece: &Piece<'_>) {
        if let Some(id) = piece.interval.session {
            self.ids.insert(id);
        }
    }

    fn value(&self) -> serde_json::Value {
        self.ids.len().into()
    }
}
