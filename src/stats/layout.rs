//! Layout: rows arranged for display. One dimension is a list, two a grid
//! (first = rows, second = columns), three or more one grid per combination
//! of the outer keys. Pure: the numbers are minutes, the text comes from
//! `label`, so the CLI and the TUI say the same.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use chrono::{Datelike, TimeDelta, TimeZone};

use crate::{
    model::time::Time,
    stats::{KeyPart, NodeKey, Row, Rows, dims::calendar::Unit, minutes_of},
};

/// A week label's date: `Sat 24 Oct`. Also the span titles' date.
pub const DAY_MONTH: &str = "%a %-d %b";

/// What a query shows. `Total` when the query has no dimensions: the total
/// is the only number.
#[derive(Debug, Clone)]
pub enum Layout {
    List { dim: &'static str, rows: Vec<Row> },
    Grid(Grid),
    Grids(Vec<(Vec<KeyPart>, Grid)>),
    Total,
}

/// A table of minutes. `cells[r][c]` is row `rows[r]` crossed with column
/// `cols[c]`, `0` where the data has no such pair. Totals are minutes of the
/// exact seconds, so they can differ from the sum of the rounded cells by up
/// to one minute per row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Grid {
    pub rows: Vec<KeyPart>,
    pub cols: Vec<KeyPart>,
    pub cells: Vec<Vec<u32>>,
    pub row_totals: Vec<u32>,
    pub col_totals: Vec<u32>,
    pub total: u32,
}

/// The layout of `rows`, from its query's dimensions.
pub fn layout(rows: &Rows) -> Layout {
    let dims = rows.query.dims.len();
    match dims {
        0 => Layout::Total,
        1 => Layout::List {
            dim: rows.query.dims[0].name(),
            rows: rows.rows.clone(),
        },
        2 => {
            let all: Vec<&Row> = rows.rows.iter().collect();
            Layout::Grid(grid(&all))
        }
        _ => {
            // Rows are sorted by key, so the outer keys group in order.
            let mut groups: BTreeMap<Vec<KeyPart>, Vec<&Row>> = BTreeMap::new();
            for row in &rows.rows {
                groups
                    .entry(row.key.0[..dims - 2].to_vec())
                    .or_default()
                    .push(row);
            }
            Layout::Grids(
                groups
                    .into_iter()
                    .map(|(outer, group)| (outer, grid(&group)))
                    .collect(),
            )
        }
    }
}

/// The grid of `rows`, whose keys end with the two grid parts: rows, then
/// columns.
fn grid(rows: &[&Row]) -> Grid {
    let parts: Vec<(&KeyPart, &KeyPart, i64)> = rows
        .iter()
        .map(|row| {
            let n = row.key.0.len();
            (&row.key.0[n - 2], &row.key.0[n - 1], row.seconds)
        })
        .collect();
    let row_keys: Vec<KeyPart> = parts
        .iter()
        .map(|&(row, _, _)| row)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .cloned()
        .collect();
    let col_keys: Vec<KeyPart> = parts
        .iter()
        .map(|&(_, col, _)| col)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .cloned()
        .collect();
    let seconds: HashMap<(&KeyPart, &KeyPart), i64> =
        parts.iter().map(|&(row, col, s)| ((row, col), s)).collect();

    let cells: Vec<Vec<u32>> = row_keys
        .iter()
        .map(|row| {
            col_keys
                .iter()
                .map(|col| minutes_of(seconds.get(&(row, col)).copied().unwrap_or(0)).get())
                .collect()
        })
        .collect();
    let row_totals: Vec<u32> = row_keys
        .iter()
        .map(|row| sum_minutes(parts.iter().filter(|p| p.0 == row).map(|p| p.2)))
        .collect();
    let col_totals: Vec<u32> = col_keys
        .iter()
        .map(|col| sum_minutes(parts.iter().filter(|p| p.1 == col).map(|p| p.2)))
        .collect();
    Grid {
        rows: row_keys,
        cols: col_keys,
        cells,
        row_totals,
        col_totals,
        total: sum_minutes(parts.iter().map(|p| p.2)),
    }
}

/// Minutes of the sum of `seconds`, summed exactly before rounding.
fn sum_minutes(seconds: impl Iterator<Item = i64>) -> u32 {
    minutes_of(seconds.sum()).get()
}

/// The text of one key part. `unit` is the calendar unit of a calendar part
/// (`None` for the others). A calendar part is named by its local start
/// date, which for an edge bucket cut to the span is the span's first day,
/// not the unit's. Root is `(root)`, removed tasks are `<name> (removed)`.
pub fn label<Tz: TimeZone>(part: &KeyPart, unit: Option<Unit>, tz: &Tz) -> String {
    match part {
        KeyPart::Calendar { start, .. } => {
            let unit = unit.unwrap_or(Unit::Day);
            let day = start.with_timezone(tz).date_naive();
            let format = match unit {
                Unit::Day => "%a %-d",
                Unit::Week => DAY_MONTH,
                Unit::Month => "%b %Y",
                Unit::Year => "%Y",
            };
            day.format(format).to_string()
        }
        KeyPart::Hour(hour) => hour.to_string(),
        KeyPart::Weekday(day) => day.to_string(),
        KeyPart::Node(NodeKey::Node { path, .. }) if path.is_empty() => "(root)".into(),
        KeyPart::Node(NodeKey::Node { path, .. }) => path.clone(),
        KeyPart::Node(NodeKey::RemovedTask { name, .. }) => format!("{name} (removed)"),
        KeyPart::Node(NodeKey::Removed) => "(removed)".into(),
    }
}

/// `label`, but a day label of a span over two months names the month too:
/// `Sun 1 Mar` in a Feb–Mar span, `Sun 1` in a span inside one month. The
/// months of `from` and of `to` minus a second (`to` is excluded).
pub fn label_in_span<Tz: TimeZone>(
    part: &KeyPart,
    unit: Option<Unit>,
    from: Time,
    to: Time,
    tz: &Tz,
) -> String {
    let first = from.with_timezone(tz).date_naive();
    let last = (to - TimeDelta::seconds(1)).with_timezone(tz).date_naive();
    let months_differ = (first.year(), first.month()) != (last.year(), last.month());
    match part {
        KeyPart::Calendar { start, .. } if unit == Some(Unit::Day) && months_differ => start
            .with_timezone(tz)
            .date_naive()
            .format(DAY_MONTH)
            .to_string(),
        _ => label(part, unit, tz),
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use chrono_tz::Europe::Berlin;
    use serde_json::json;

    use super::*;
    use crate::{
        model::{
            id::NodeId,
            sessions::{SessionId, TaskRef},
        },
        stats::{Ctx, DimSpec, Interval, Key, MeasureSpec, Query, SourceKind, run},
        test_util::{parse_time, tree_with},
    };

    /// A tracked interval on one task, in a session of its own.
    fn iv(start: &str, end: &str) -> Interval {
        Interval {
            start: parse_time(start),
            end: parse_time(end),
            task: TaskRef {
                id: NodeId::new(),
                name: "a".into(),
                description: String::new(),
                container_dir: PathBuf::from("/uni"),
                container_id: NodeId::new(),
            },
            session: Some(SessionId::new()),
            source: SourceKind::Tracked,
        }
    }

    /// `run` over `from`..`to` with the given dimensions, in Berlin.
    fn rows(dims: Vec<DimSpec>, from: &str, to: &str, intervals: &[Interval]) -> Rows {
        let query = Query {
            from: parse_time(from),
            to: parse_time(to),
            dims,
            measures: vec![MeasureSpec::Minutes, MeasureSpec::Sessions],
            include_removed_tasks: false,
        };
        let tree = tree_with(vec![]);
        let ctx = Ctx {
            tree: &tree,
            scope: &[],
            tz: Berlin,
        };
        run(&query, intervals, &ctx)
    }

    /// A calendar part starting at `start`; the end is not shown by `label`.
    fn calendar(start: &str) -> KeyPart {
        KeyPart::Calendar {
            start: parse_time(start),
            end: parse_time(start),
        }
    }

    #[test]
    fn one_dim_is_list() {
        let intervals = [iv("2026-10-27T09:00:00+01:00", "2026-10-27T10:00:00+01:00")];
        let rows = rows(
            vec![DimSpec::Weekday],
            "2026-10-26T00:00:00+01:00",
            "2026-11-02T00:00:00+01:00",
            &intervals,
        );
        let Layout::List { dim, rows: list } = layout(&rows) else {
            panic!("one dimension is a list");
        };
        assert_eq!(dim, "weekday");
        assert_eq!(list.len(), 7);
        // Tuesday.
        assert_eq!(list[1].values["minutes"], json!(60));
        assert_eq!(
            list[1].key,
            Key(vec![KeyPart::Weekday(chrono::Weekday::Tue)])
        );
    }

    #[test]
    fn two_dims_grid_totals() {
        let intervals = [
            iv("2026-10-26T09:30:00+01:00", "2026-10-26T10:00:00+01:00"),
            iv("2026-10-27T09:00:00+01:00", "2026-10-27T10:00:00+01:00"),
        ];
        let rows = rows(
            vec![DimSpec::Weekday, DimSpec::Hour],
            "2026-10-26T00:00:00+01:00",
            "2026-11-02T00:00:00+01:00",
            &intervals,
        );
        let Layout::Grid(grid) = layout(&rows) else {
            panic!("two dimensions are a grid");
        };
        assert_eq!(grid.rows.len(), 7);
        assert_eq!(grid.cols.len(), 24);
        assert_eq!(grid.cells[0][9], 30);
        assert_eq!(grid.cells[1][9], 60);
        assert_eq!(grid.cells[1][10], 0);
        assert_eq!(grid.row_totals, vec![30, 60, 0, 0, 0, 0, 0]);
        assert_eq!(grid.col_totals[9], 90);
        assert_eq!(grid.total, 90);
        assert_eq!(grid.row_totals.iter().sum::<u32>(), grid.total);
        assert_eq!(grid.col_totals.iter().sum::<u32>(), grid.total);
    }

    #[test]
    fn three_dims_grids() {
        let intervals = [
            iv("2026-10-19T09:00:00+02:00", "2026-10-19T10:00:00+02:00"),
            iv("2026-10-26T09:00:00+01:00", "2026-10-26T09:30:00+01:00"),
        ];
        let rows = rows(
            vec![
                DimSpec::Calendar {
                    unit: Unit::Week,
                    every: 1,
                },
                DimSpec::Weekday,
                DimSpec::Hour,
            ],
            "2026-10-19T00:00:00+02:00",
            "2026-11-02T00:00:00+01:00",
            &intervals,
        );
        let Layout::Grids(grids) = layout(&rows) else {
            panic!("three dimensions are grids");
        };
        assert_eq!(grids.len(), 2);
        let week = |outer: &[KeyPart]| label(&outer[0], Some(Unit::Week), &Berlin);
        assert_eq!(week(&grids[0].0), "Mon 19 Oct");
        assert_eq!(week(&grids[1].0), "Mon 26 Oct");
        let (first, second) = (&grids[0].1, &grids[1].1);
        assert_eq!(first.rows.len(), 7);
        assert_eq!(first.cols.len(), 24);
        assert_eq!(first.cells[0][9], 60);
        assert_eq!(second.cells[0][9], 30);
        assert_eq!(first.total, 60);
        assert_eq!(second.total, 30);
    }

    #[test]
    fn no_dims_is_total() {
        let rows = rows(
            vec![],
            "2026-10-26T00:00:00+01:00",
            "2026-11-02T00:00:00+01:00",
            &[],
        );
        assert!(matches!(layout(&rows), Layout::Total));
    }

    #[test]
    fn labels() {
        let text = |part: KeyPart, unit: Option<Unit>| label(&part, unit, &Berlin);

        assert_eq!(
            text(calendar("2026-10-24T00:00:00+02:00"), Some(Unit::Day)),
            "Sat 24"
        );
        assert_eq!(
            text(calendar("2026-10-19T00:00:00+02:00"), Some(Unit::Week)),
            "Mon 19 Oct"
        );
        // An edge bucket cut to a span that starts mid-week: its start day.
        assert_eq!(
            text(calendar("2026-10-21T10:00:00+02:00"), Some(Unit::Week)),
            "Wed 21 Oct"
        );
        assert_eq!(
            text(calendar("2026-10-01T00:00:00+02:00"), Some(Unit::Month)),
            "Oct 2026"
        );
        assert_eq!(
            text(calendar("2026-01-01T00:00:00+01:00"), Some(Unit::Year)),
            "2026"
        );

        assert_eq!(text(KeyPart::Hour(0), None), "0");
        assert_eq!(text(KeyPart::Hour(23), None), "23");
        assert_eq!(text(KeyPart::Weekday(chrono::Weekday::Mon), None), "Mon");

        let node = |path: &str| {
            KeyPart::Node(NodeKey::Node {
                id: NodeId::new(),
                path: path.into(),
                order: 0,
            })
        };
        assert_eq!(text(node(""), None), "(root)");
        assert_eq!(text(node("uni/cs"), None), "uni/cs");
        let removed = KeyPart::Node(NodeKey::RemovedTask {
            id: NodeId::new(),
            name: "lab 3".into(),
        });
        assert_eq!(text(removed, None), "lab 3 (removed)");
        assert_eq!(text(KeyPart::Node(NodeKey::Removed), None), "(removed)");
    }

    #[test]
    fn span_labels_name_the_month_across_months() {
        let text = |part: KeyPart, unit: Option<Unit>, from: &str, to: &str| {
            label_in_span(&part, unit, parse_time(from), parse_time(to), &Berlin)
        };

        // Inside one month: the day alone.
        assert_eq!(
            text(
                calendar("2026-10-24T00:00:00+02:00"),
                Some(Unit::Day),
                "2026-10-20T00:00:00+02:00",
                "2026-10-25T00:00:00+02:00",
            ),
            "Sat 24"
        );
        // Feb to Mar: the first of March carries its month.
        assert_eq!(
            text(
                calendar("2026-03-01T00:00:00+01:00"),
                Some(Unit::Day),
                "2026-02-01T00:00:00+01:00",
                "2026-03-02T00:00:00+01:00",
            ),
            "Sun 1 Mar"
        );
        // Dec to Jan: crossing a year is crossing a month.
        assert_eq!(
            text(
                calendar("2026-01-01T00:00:00+01:00"),
                Some(Unit::Day),
                "2025-12-20T00:00:00+01:00",
                "2026-01-05T00:00:00+01:00",
            ),
            "Thu 1 Jan"
        );
        // `to` is exclusive: its last second is 31 Oct, still one month.
        assert_eq!(
            text(
                calendar("2026-10-30T00:00:00+01:00"),
                Some(Unit::Day),
                "2026-10-01T00:00:00+02:00",
                "2026-11-01T00:00:00+01:00",
            ),
            "Fri 30"
        );
        // Other units and other parts: exactly `label`.
        assert_eq!(
            text(
                calendar("2026-02-23T00:00:00+01:00"),
                Some(Unit::Week),
                "2026-02-01T00:00:00+01:00",
                "2026-03-02T00:00:00+01:00",
            ),
            label(
                &calendar("2026-02-23T00:00:00+01:00"),
                Some(Unit::Week),
                &Berlin
            )
        );
        assert_eq!(
            text(
                KeyPart::Hour(9),
                None,
                "2026-02-01T00:00:00+01:00",
                "2026-03-02T00:00:00+01:00",
            ),
            "9"
        );
    }
}
