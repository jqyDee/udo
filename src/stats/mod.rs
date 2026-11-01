//! Stats: time grouped by any dimensions and counted by any measures. Pure,
//! like `estimate`: no `Core`, no I/O. The zone and `now` come in; `run`
//! takes intervals and returns rows.

pub mod dims;
pub mod interval;
pub mod layout;
pub mod measures;
pub mod sources;
pub mod spec;
#[cfg(test)]
mod tests;

use std::{
    cmp::Ordering,
    collections::{BTreeMap, BTreeSet, HashMap},
    path::PathBuf,
};

use chrono::{TimeZone, Weekday};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::model::{
    id::NodeId,
    sessions::TaskRef,
    time::{Minutes, Time},
    tree::Tree,
};

pub use dims::Dim;
pub use interval::{Interval, Piece, SourceKind};
pub use measures::{Acc, Measure};
pub use spec::{DimSpec, MeasureSpec};

use interval::cut;

/// Where the query stands in the tree: the tree, the scope path (node keys
/// need ancestors and names) and the zone.
pub struct Ctx<'a, Tz: TimeZone> {
    pub tree: &'a Tree,
    pub scope: &'a [usize],
    pub tz: Tz,
}

/// One value of one dimension. A `Key` has one per dimension, in query order.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KeyPart {
    /// Calendar: one bucket, `start` to `end` (cut to the span at the edges).
    Calendar {
        start: Time,
        end: Time,
    },
    /// Hour of the local day, 0..=23.
    Hour(u8),
    Weekday(Weekday),
    Node(NodeKey),
}

/// A node key: a node of the tree, a removed task, or the removed tasks of the
/// coarser levels in one key.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeKey {
    /// `order` is the node's row in the tree (for sorting), not stored: a key
    /// read back has 0.
    Node {
        id: NodeId,
        path: String,
        #[serde(skip)]
        order: usize,
    },
    RemovedTask {
        id: NodeId,
        name: String,
    },
    Removed,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Key(pub Vec<KeyPart>);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Query {
    pub from: Time,
    pub to: Time,
    pub dims: Vec<DimSpec>,
    pub measures: Vec<MeasureSpec>,
    /// Sessions of removed tasks too (`--deleted`). Not the sessions you
    /// deleted: those never count.
    pub include_removed_tasks: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Row {
    pub key: Key,
    pub values: BTreeMap<&'static str, Value>,
    pub occurrences: u32,
    /// The exact sum in seconds; `layout` reads it, JSON never shows it.
    #[serde(skip)]
    pub seconds: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Rows {
    pub query: Query,
    pub rows: Vec<Row>,
    pub total: Row,
}

/// Whole minutes of `seconds`, rounded down, never below 0. The one rounding
/// rule of udo: swapping it (#85) means changing this function only.
pub fn minutes_of(seconds: i64) -> Minutes {
    Minutes::new(u32::try_from(seconds.max(0) / 60).unwrap_or(u32::MAX))
}

/// Runs `query` over `intervals`, which the caller has already scoped and
/// filtered. Rows come in display order; `total` covers the whole span.
pub fn run<Tz: TimeZone + 'static>(query: &Query, intervals: &[Interval], ctx: &Ctx<Tz>) -> Rows {
    let dims: Vec<Box<dyn Dim<Tz>>> = query.dims.iter().map(|d| d.build()).collect();
    let measures: Vec<Box<dyn Measure>> = query.measures.iter().map(MeasureSpec::build).collect();
    run_with(query, &dims, &measures, intervals, ctx)
}

/// `run` with its dimensions and measures given, so tests can use a dimension
/// no `DimSpec` names yet.
fn run_with<Tz: TimeZone>(
    query: &Query,
    dims: &[Box<dyn Dim<Tz>>],
    measures: &[Box<dyn Measure>],
    intervals: &[Interval],
    ctx: &Ctx<Tz>,
) -> Rows {
    let mut cuts: Vec<Time> = dims
        .iter()
        .flat_map(|d| d.cuts(query.from, query.to, &ctx.tz))
        .collect();
    cuts.sort();
    cuts.dedup();
    let pieces = cut(intervals, query.from, query.to, &cuts);

    let mut total = Group::new(measures);
    let mut groups: BTreeMap<Key, Group> = BTreeMap::new();
    for piece in &pieces {
        total.add(piece);
        let key = Key(dims
            .iter()
            .map(|d| d.key(piece, query.from, query.to, ctx))
            .collect());
        groups
            .entry(key)
            .or_insert_with(|| Group::new(measures))
            .add(piece);
    }

    // Without dimensions there are no rows, only the total.
    let mut shown = BTreeMap::new();
    if !dims.is_empty() {
        let enumerated: Vec<_> = dims
            .iter()
            .map(|d| d.keys_in(query.from, query.to, &ctx.tz))
            .collect();
        let observed: Vec<&Key> = groups.keys().collect();
        shown = occurrences(&enumerated, &observed);
        // With 2+ enumerating dimensions the product in `occurrences` is wrong
        // (it is computed, then replaced here): count their keys jointly.
        let enumerating: Vec<usize> = enumerated
            .iter()
            .enumerate()
            .filter_map(|(i, keys)| keys.is_some().then_some(i))
            .collect();
        if enumerating.len() >= 2 {
            let joint = joint_occurrences(dims, &enumerating, query, ctx);
            for (key, occurrences) in &mut shown {
                let parts: Vec<KeyPart> = enumerating.iter().map(|&i| key.0[i].clone()).collect();
                *occurrences = joint.get(&parts).copied().unwrap_or(0);
            }
        }
        // Observed keys no dimension enumerates still show, once.
        for key in observed {
            shown.entry(key.clone()).or_insert(1);
        }
    }
    let rows = shown
        .into_iter()
        .map(|(key, occurrences)| {
            let group = groups.remove(&key).unwrap_or_else(|| Group::new(measures));
            group.into_row(key, occurrences)
        })
        .collect();

    Rows {
        query: query.clone(),
        rows,
        total: total.into_row(Key(Vec::new()), 1),
    }
}

/// The keys to show, with their occurrences: the cross product of the
/// enumerating dimensions' keys, crossed with the observed combinations of
/// the others (if there are any; then only those). The occurrences are the
/// product of the keys' own counts, which is right for 0 or 1 enumerating
/// dimension only; `run_with` recounts for 2+.
fn occurrences(
    enumerated: &[Option<Vec<(KeyPart, u32)>>],
    observed: &[&Key],
) -> BTreeMap<Key, u32> {
    let mut out = BTreeMap::new();
    // The observed combinations of the others, each once: observed keys that
    // agree on the others cross the same keys, so they give the same rows.
    // All dimensions enumerate: one base, no observed combinations needed.
    let bases: BTreeSet<Vec<Option<KeyPart>>> = if enumerated.iter().all(Option::is_some) {
        BTreeSet::from([vec![None; enumerated.len()]])
    } else {
        observed
            .iter()
            .map(|key| {
                key.0
                    .iter()
                    .zip(enumerated)
                    .map(|(part, keys)| keys.is_none().then(|| part.clone()))
                    .collect()
            })
            .collect()
    };
    for base in bases {
        let slots: Vec<Vec<(KeyPart, u32)>> = enumerated
            .iter()
            .zip(&base)
            .map(|(keys, part)| match (keys, part) {
                (Some(keys), _) => keys.clone(),
                (None, Some(part)) => vec![(part.clone(), 1)],
                (None, None) => unreachable!("a base is only missing when all dims enumerate"),
            })
            .collect();
        product(&slots, &mut out);
    }
    out
}

/// Adds every combination of one entry per slot to `out`, with the product
/// of the occurrences.
fn product(slots: &[Vec<(KeyPart, u32)>], out: &mut BTreeMap<Key, u32>) {
    let mut partial = vec![(Vec::new(), 1u32)];
    for slot in slots {
        partial = partial
            .iter()
            .flat_map(|(parts, occurrences)| {
                slot.iter().map(move |(part, n)| {
                    let mut parts: Vec<KeyPart> = parts.clone();
                    parts.push(part.clone());
                    (parts, occurrences * n)
                })
            })
            .collect();
    }
    for (parts, occurrences) in partial {
        out.insert(Key(parts), occurrences);
    }
}

/// How often each combination of the enumerating dimensions' keys occurs in
/// the span, counted jointly: the span is cut at their cuts and each slot
/// counts once for the combination of keys it has. Not a product of the
/// dimensions' own counts: (Mon, 9h) in October is 4, not 4 x 31. Equal slots
/// next to each other stay apart, so a repeated DST hour counts twice.
fn joint_occurrences<Tz: TimeZone>(
    dims: &[Box<dyn Dim<Tz>>],
    enumerating: &[usize],
    query: &Query,
    ctx: &Ctx<Tz>,
) -> HashMap<Vec<KeyPart>, u32> {
    let (from, to) = (query.from, query.to);
    let mut counts = HashMap::new();
    if from >= to {
        return counts;
    }
    let mut cuts: Vec<Time> = enumerating
        .iter()
        .flat_map(|&i| dims[i].cuts(from, to, &ctx.tz))
        .collect();
    cuts.sort();
    cuts.dedup();
    // Only the start of a piece matters to the time dimensions' keys (see
    // `Dim::key`), so the task is a placeholder that is never read.
    let interval = Interval {
        start: from,
        end: to,
        task: TaskRef {
            id: NodeId::NIL,
            name: String::new(),
            description: String::new(),
            container_dir: PathBuf::new(),
            container_id: NodeId::NIL,
        },
        session: None,
        source: SourceKind::Tracked,
    };
    let starts = std::iter::once(from).chain(cuts.iter().copied());
    let ends = cuts.iter().copied().chain(std::iter::once(to));
    for (start, end) in starts.zip(ends) {
        let piece = Piece {
            interval: &interval,
            start,
            end,
        };
        let parts: Vec<KeyPart> = enumerating
            .iter()
            .map(|&i| dims[i].key(&piece, from, to, ctx))
            .collect();
        *counts.entry(parts).or_insert(0) += 1;
    }
    counts
}

/// The accumulators of one group (or of the total).
struct Group {
    seconds: i64,
    accs: Vec<(&'static str, Box<dyn Acc>)>,
}

impl Group {
    fn new(measures: &[Box<dyn Measure>]) -> Self {
        Self {
            seconds: 0,
            accs: measures.iter().map(|m| (m.name(), m.new_acc())).collect(),
        }
    }

    fn add(&mut self, piece: &Piece<'_>) {
        self.seconds += piece.seconds();
        for (_, acc) in &mut self.accs {
            acc.add(piece);
        }
    }

    fn into_row(self, key: Key, occurrences: u32) -> Row {
        Row {
            key,
            values: self
                .accs
                .iter()
                .map(|(name, acc)| (*name, acc.value()))
                .collect(),
            occurrences,
            seconds: self.seconds,
        }
    }
}

impl KeyPart {
    /// Position of the variant: the order between different kinds.
    fn rank(&self) -> u8 {
        match self {
            KeyPart::Calendar { .. } => 0,
            KeyPart::Hour(_) => 1,
            KeyPart::Weekday(_) => 2,
            KeyPart::Node(_) => 3,
        }
    }
}

/// Display order: calendar chronological, hours 0-23, weekdays Mon-Sun, nodes
/// in tree order. `chrono::Weekday` has no `Ord`, so this is written out.
impl Ord for KeyPart {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self, other) {
            (
                KeyPart::Calendar { start, end },
                KeyPart::Calendar {
                    start: other_start,
                    end: other_end,
                },
            ) => (start, end).cmp(&(other_start, other_end)),
            (KeyPart::Hour(a), KeyPart::Hour(b)) => a.cmp(b),
            (KeyPart::Weekday(a), KeyPart::Weekday(b)) => {
                a.num_days_from_monday().cmp(&b.num_days_from_monday())
            }
            (KeyPart::Node(a), KeyPart::Node(b)) => a.cmp(b),
            // Every variant is listed, so a new one fails to compile here
            // instead of comparing as equal to another kind.
            (
                KeyPart::Calendar { .. }
                | KeyPart::Hour(_)
                | KeyPart::Weekday(_)
                | KeyPart::Node(_),
                _,
            ) => self.rank().cmp(&other.rank()),
        }
    }
}

impl PartialOrd for KeyPart {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Tree order, then removed tasks by name, then `Removed`. Nodes compare by
/// `order`, id, then path, which keeps `Ord` consistent with `Eq`.
impl Ord for NodeKey {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self, other) {
            (
                NodeKey::Node {
                    id, order, path, ..
                },
                NodeKey::Node {
                    id: other_id,
                    order: other_order,
                    path: other_path,
                    ..
                },
            ) => (order, id, path).cmp(&(other_order, other_id, other_path)),
            (
                NodeKey::RemovedTask { id, name },
                NodeKey::RemovedTask {
                    id: other_id,
                    name: other_name,
                },
            ) => (name, id).cmp(&(other_name, other_id)),
            // Every variant is listed, as in `KeyPart`'s `Ord`.
            (NodeKey::Node { .. } | NodeKey::RemovedTask { .. } | NodeKey::Removed, _) => {
                self.rank().cmp(&other.rank())
            }
        }
    }
}

impl PartialOrd for NodeKey {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl NodeKey {
    fn rank(&self) -> u8 {
        match self {
            NodeKey::Node { .. } => 0,
            NodeKey::RemovedTask { .. } => 1,
            NodeKey::Removed => 2,
        }
    }
}
