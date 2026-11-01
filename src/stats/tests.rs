use std::path::PathBuf;

use chrono::{SubsecRound, TimeDelta, TimeZone};
use chrono_tz::{Europe::Berlin, Tz};
use proptest::prelude::*;
use serde_json::json;

use super::*;
use crate::{
    model::{
        id::NodeId,
        sessions::{SessionId, TaskRef},
        time::{Minutes, Time},
    },
    stats::{dims::calendar::Unit, spec::Level},
    test_util::{parse_time, task, tree_with, uni_tree},
};

/// A tracked interval on task `name`, in a session of its own.
fn iv(start: &str, end: &str, name: &str) -> Interval {
    Interval {
        start: parse_time(start),
        end: parse_time(end),
        task: TaskRef {
            id: NodeId::new(),
            name: name.into(),
            description: String::new(),
            container_dir: PathBuf::from("/uni"),
            container_id: NodeId::new(),
        },
        session: Some(SessionId::new()),
        source: SourceKind::Tracked,
    }
}

/// Minutes and sessions over the span `from`..`to`, no dimensions.
fn query(from: &str, to: &str) -> Query {
    Query {
        from: parse_time(from),
        to: parse_time(to),
        dims: vec![],
        measures: vec![MeasureSpec::Minutes, MeasureSpec::Sessions],
        include_removed_tasks: false,
    }
}

/// `run` in the test zone (Berlin: DST days are real), with an empty tree.
fn go(query: &Query, intervals: &[Interval]) -> Rows {
    let tree = tree_with(vec![]);
    let ctx = Ctx {
        tree: &tree,
        scope: &[],
        tz: Berlin,
    };
    run(query, intervals, &ctx)
}

/// Cuts at the given instants. Each piece is keyed by its own bounds. A
/// dimension no `DimSpec` names yet, so the cutting is tested alone.
struct Boundaries(Vec<Time>);

impl<Tz: TimeZone> Dim<Tz> for Boundaries {
    fn name(&self) -> &'static str {
        "boundaries"
    }

    fn cuts(&self, _from: Time, _to: Time, _tz: &Tz) -> Vec<Time> {
        self.0.clone()
    }

    fn key(&self, piece: &Piece<'_>, _from: Time, _to: Time, _ctx: &Ctx<'_, Tz>) -> KeyPart {
        KeyPart::Calendar {
            start: piece.start,
            end: piece.end,
        }
    }

    fn keys_in(&self, _from: Time, _to: Time, _tz: &Tz) -> Option<Vec<(KeyPart, u32)>> {
        None
    }
}

#[test]
fn empty_query_total_only() {
    let q = query("2026-10-15T00:00:00+02:00", "2026-10-16T00:00:00+02:00");
    let intervals = [
        iv(
            "2026-10-15T09:00:00+02:00",
            "2026-10-15T10:00:00+02:00",
            "a",
        ),
        iv(
            "2026-10-15T11:00:00+02:00",
            "2026-10-15T11:30:00+02:00",
            "b",
        ),
    ];
    let rows = go(&q, &intervals);
    assert!(rows.rows.is_empty());
    assert_eq!(rows.total.values["minutes"], json!(90));
    assert_eq!(rows.total.occurrences, 1);
}

#[test]
fn clip_to_span() {
    let q = query("2026-10-15T10:00:00+02:00", "2026-10-15T12:00:00+02:00");
    let intervals = [iv(
        "2026-10-15T09:00:00+02:00",
        "2026-10-15T11:00:00+02:00",
        "a",
    )];
    let rows = go(&q, &intervals);
    assert_eq!(rows.total.values["minutes"], json!(60));
}

#[test]
fn zero_length_dropped() {
    let q = query("2026-10-15T00:00:00+02:00", "2026-10-16T00:00:00+02:00");
    let intervals = [
        iv(
            "2026-10-15T10:00:00+02:00",
            "2026-10-15T10:00:00+02:00",
            "a",
        ),
        iv(
            "2026-10-15T11:00:00+02:00",
            "2026-10-15T09:00:00+02:00",
            "b",
        ),
    ];
    let rows = go(&q, &intervals);
    assert_eq!(rows.total.seconds, 0);
    assert_eq!(rows.total.values["minutes"], json!(0));
    assert_eq!(rows.total.values["sessions"], json!(0));
}

#[test]
fn running_session_counts_to_now() {
    // The caller passes `now` as the end of a running session.
    let running = [iv(
        "2026-10-15T09:00:00+02:00",
        "2026-10-15T11:30:00+02:00",
        "a",
    )];
    let q = query("2026-10-15T10:00:00+02:00", "2026-10-15T12:00:00+02:00");
    assert_eq!(go(&q, &running).total.values["minutes"], json!(90));
    // Never past `to`, even if `now` is later.
    let q = query("2026-10-15T10:00:00+02:00", "2026-10-15T11:00:00+02:00");
    assert_eq!(go(&q, &running).total.values["minutes"], json!(60));
}

#[test]
fn split_at_midnight() {
    let q = query("2026-10-15T00:00:00+02:00", "2026-10-17T00:00:00+02:00");
    let tree = tree_with(vec![]);
    let ctx = Ctx {
        tree: &tree,
        scope: &[],
        tz: Berlin,
    };
    let dims: Vec<Box<dyn Dim<Tz>>> = vec![Box::new(Boundaries(vec![parse_time(
        "2026-10-16T00:00:00+02:00",
    )]))];
    let measures: Vec<Box<dyn Measure>> = q.measures.iter().map(MeasureSpec::build).collect();
    let intervals = [iv(
        "2026-10-15T23:00:00+02:00",
        "2026-10-16T01:00:00+02:00",
        "a",
    )];
    let rows = run_with(&q, &dims, &measures, &intervals, &ctx);
    assert_eq!(rows.rows.len(), 2);
    assert!(rows.rows.iter().all(|r| r.values["minutes"] == json!(60)));
    // Counted on both days, once in the total.
    assert!(rows.rows.iter().all(|r| r.values["sessions"] == json!(1)));
    assert_eq!(rows.total.values["minutes"], json!(120));
    assert_eq!(rows.total.values["sessions"], json!(1));
}

#[test]
fn node_by_day_and_hour_crosses_each_node_once() {
    // Three tasks, each seen on some days: each one crosses the whole day x
    // hour grid once, however often it was seen.
    let tree = tree_with(vec![task("a"), task("b"), task("c")]);
    let tasks = tree_tasks(&tree);
    let intervals: Vec<Interval> = (0..30i64)
        .map(|day| {
            let start = parse_time("2026-10-01T09:00:00+02:00") + TimeDelta::days(day);
            Interval {
                start,
                end: start + TimeDelta::hours(1),
                task: tasks[day as usize % tasks.len()].clone(),
                session: Some(SessionId::new()),
                source: SourceKind::Tracked,
            }
        })
        .collect();
    let query = Query {
        from: parse_time("2026-10-01T00:00:00+02:00"),
        to: parse_time("2026-10-31T00:00:00+01:00"),
        dims: vec![
            DimSpec::Node { level: Level::Task },
            DimSpec::Calendar {
                unit: Unit::Day,
                every: 1,
            },
            DimSpec::Hour,
        ],
        measures: vec![MeasureSpec::Minutes, MeasureSpec::Sessions],
        include_removed_tasks: false,
    };
    let ctx = Ctx {
        tree: &tree,
        scope: &[],
        tz: Berlin,
    };
    let rows = run(&query, &intervals, &ctx);

    // 3 tasks x 30 days x 24 hours. A repeated DST hour is still one key.
    assert_eq!(rows.rows.len(), 3 * 30 * 24);
    assert_eq!(rows.total.values["minutes"], json!(30 * 60));
}

#[test]
fn split_keeps_the_total_with_subseconds() {
    // Session times keep milliseconds; the cut is at whole seconds.
    let q = query("2026-10-15T00:00:00+02:00", "2026-10-17T00:00:00+02:00");
    let tree = tree_with(vec![]);
    let ctx = Ctx {
        tree: &tree,
        scope: &[],
        tz: Berlin,
    };
    let intervals = [iv(
        "2026-10-15T23:00:00.600+02:00",
        "2026-10-16T01:00:00.700+02:00",
        "a",
    )];
    let measures: Vec<Box<dyn Measure>> = q.measures.iter().map(MeasureSpec::build).collect();
    let uncut = run_with::<Tz>(&q, &[], &measures, &intervals, &ctx);
    let dims: Vec<Box<dyn Dim<Tz>>> = vec![Box::new(Boundaries(vec![parse_time(
        "2026-10-16T00:00:00+02:00",
    )]))];
    let cut = run_with(&q, &dims, &measures, &intervals, &ctx);
    assert_eq!(uncut.total.seconds, 7200);
    assert_eq!(cut.total.seconds, uncut.total.seconds);
    assert_eq!(cut.total.values["minutes"], uncut.total.values["minutes"]);
}

#[test]
fn sessions_measure_counts_distinct() {
    let q = query("2026-10-15T00:00:00+02:00", "2026-10-16T00:00:00+02:00");
    let shared = SessionId::new();
    let first = Interval {
        session: Some(shared),
        ..iv(
            "2026-10-15T09:00:00+02:00",
            "2026-10-15T10:00:00+02:00",
            "a",
        )
    };
    let second = Interval {
        session: Some(shared),
        ..iv(
            "2026-10-15T11:00:00+02:00",
            "2026-10-15T12:00:00+02:00",
            "a",
        )
    };
    let rows = go(&q, &[first, second]);
    assert_eq!(rows.total.values["sessions"], json!(1));
}

#[test]
fn minutes_round_down() {
    assert_eq!(minutes_of(119), Minutes::new(1));
    assert_eq!(minutes_of(59), Minutes::new(0));
    assert_eq!(minutes_of(-5), Minutes::new(0));
}

// ---------- invariant: grouping never changes the total ----------

const DAY: i64 = 86_400;

/// Every `DimSpec` a query can name. A new dimension must be added here, or
/// the invariant below does not cover it.
fn any_dim() -> impl Strategy<Value = DimSpec> {
    let unit = prop_oneof![
        Just(Unit::Day),
        Just(Unit::Week),
        Just(Unit::Month),
        Just(Unit::Year),
    ];
    let level = prop_oneof![
        Just(Level::Task),
        Just(Level::Project),
        Just(Level::Workspace),
        Just(Level::Child),
    ];
    prop_oneof![
        (unit, 1..=6u32).prop_map(|(unit, every)| DimSpec::Calendar { unit, every }),
        Just(DimSpec::Hour),
        Just(DimSpec::Weekday),
        level.prop_map(|level| DimSpec::Node { level }),
    ]
}

/// Milliseconds from the base instant, in `secs` seconds (plus a random
/// sub-second part, often zero).
fn millis_within(secs: std::ops::RangeInclusive<i64>) -> impl Strategy<Value = i64> {
    (secs, prop_oneof![Just(0i64), 0..1000i64]).prop_map(|(s, ms)| s * 1000 + ms)
}

/// The base instant of a case: midnight in Berlin of one of the two DST days
/// of 2026. Spring (2026-03-29) is 23 h long and skips 02:00; autumn
/// (2026-10-25) is 25 h long and repeats an hour.
fn bases() -> impl Strategy<Value = Time> {
    prop_oneof![
        Just(parse_time("2026-03-29T00:00:00+01:00")),
        Just(parse_time("2026-10-25T00:00:00+02:00")),
    ]
}

/// The tree's tasks, each as a session would remember it.
fn tree_tasks(tree: &Tree) -> Vec<TaskRef> {
    tree.rows()
        .into_iter()
        .filter(|r| r.node.as_task().is_some())
        .map(|r| {
            let parent_path = &r.path[..r.path.len() - 1];
            TaskRef::of(r.node, tree.get(parent_path).unwrap()).unwrap()
        })
        .collect()
}

/// The clipped length in whole seconds, computed from the clipping rule
/// alone: truncate each endpoint and the span, clip, keep positive lengths.
fn clipped_seconds(intervals: &[Interval], from: Time, to: Time) -> i64 {
    let from = from.trunc_subsecs(0).timestamp();
    let to = to.trunc_subsecs(0).timestamp();
    intervals
        .iter()
        .map(|iv| {
            let start = iv.start.trunc_subsecs(0).timestamp().max(from);
            let end = iv.end.trunc_subsecs(0).timestamp().min(to);
            (end - start).max(0)
        })
        .sum()
}

proptest! {
    /// Across every dimension mix, the rows sum to the total, and the total
    /// is the clipped input. Intervals cross midnight and run up to 30 h
    /// around one of the two DST days; some end on a removed task.
    #[test]
    fn invariant_total_unchanged(
        base in bases(),
        spans in proptest::collection::vec(
            (
                millis_within(-40 * DAY..=40 * DAY),
                millis_within(0..=30 * 3600),
                any::<proptest::sample::Index>(),
            ),
            0..40,
        ),
        from_ms in millis_within(-40 * DAY..=40 * DAY),
        span_ms in millis_within(0..=60 * DAY),
        dims in proptest::collection::vec(any_dim(), 0..=3),
    ) {
        let tree = uni_tree();
        let mut tasks = tree_tasks(&tree);
        tasks.push(TaskRef {
            id: NodeId::new(),
            name: "removed".into(),
            description: String::new(),
            container_dir: PathBuf::from("/uni"),
            container_id: NodeId::new(),
        });
        let intervals: Vec<Interval> = spans
            .into_iter()
            .map(|(start_ms, len_ms, pick)| {
                let start = base + TimeDelta::milliseconds(start_ms);
                Interval {
                    start,
                    end: start + TimeDelta::milliseconds(len_ms),
                    task: tasks[pick.index(tasks.len())].clone(),
                    session: Some(SessionId::new()),
                    source: SourceKind::Tracked,
                }
            })
            .collect();
        let from = base + TimeDelta::milliseconds(from_ms);
        let to = from + TimeDelta::milliseconds(span_ms);
        let q = Query {
            from,
            to,
            dims,
            measures: vec![MeasureSpec::Minutes, MeasureSpec::Sessions],
            include_removed_tasks: true,
        };
        let ctx = Ctx {
            tree: &tree,
            scope: &[],
            tz: Berlin,
        };
        let rows = run(&q, &intervals, &ctx);

        let expected = clipped_seconds(&intervals, from, to);
        // Without dimensions there are no rows, only the total.
        if !rows.query.dims.is_empty() {
            let grouped: i64 = rows.rows.iter().map(|r| r.seconds).sum();
            prop_assert_eq!(grouped, expected);
        }
        prop_assert_eq!(rows.total.seconds, expected);
    }
}
