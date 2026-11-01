pub mod calendar;
pub mod hour;
pub mod node;
pub mod weekday;

use chrono::TimeZone;

use crate::{
    model::time::Time,
    stats::{Ctx, KeyPart, interval::Piece},
};

/// A way to group time. The zone comes in with each call (`Ctx` holds it
/// for `key`), so the engine never reads the machine's zone.
pub trait Dim<Tz: TimeZone> {
    /// Name in JSON keys and CLI flags (`"day"`, `"weekday"`, ...).
    fn name(&self) -> &'static str;
    /// Points in `(from, to)` where intervals must be cut so each piece has
    /// one key (calendar: bucket starts; hour: every local hour; weekday:
    /// every local midnight; node: none).
    fn cuts(&self, from: Time, to: Time, tz: &Tz) -> Vec<Time>;
    /// The key of a piece. `from` and `to` are the span, which a key can
    /// need (calendar: edge buckets are cut to it). A dimension whose
    /// `keys_in` is `Some` must not read `piece.interval` here: its occurrences
    /// are counted on placeholder intervals.
    fn key(&self, piece: &Piece<'_>, from: Time, to: Time, ctx: &Ctx<'_, Tz>) -> KeyPart;
    /// Every key in the span with how often it occurs, in display order;
    /// `None` if the keys come only from the data (node).
    fn keys_in(&self, from: Time, to: Time, tz: &Tz) -> Option<Vec<(KeyPart, u32)>>;
}
