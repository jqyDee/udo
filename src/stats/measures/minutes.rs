use crate::stats::{
    interval::Piece,
    measures::{Acc, Measure},
    minutes_of,
};

/// Time spent: the sum of the pieces, in seconds, shown as whole minutes.
pub struct MinutesMeasure;

impl Measure for MinutesMeasure {
    fn name(&self) -> &'static str {
        "minutes"
    }

    fn new_acc(&self) -> Box<dyn Acc> {
        Box::new(MinutesAcc { seconds: 0 })
    }
}

struct MinutesAcc {
    seconds: i64,
}

impl Acc for MinutesAcc {
    fn add(&mut self, piece: &Piece<'_>) {
        self.seconds += piece.seconds();
    }

    fn value(&self) -> serde_json::Value {
        minutes_of(self.seconds).get().into()
    }
}
