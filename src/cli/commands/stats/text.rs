//! The text form of `udo stats`: each section's heading with its total, then
//! its parts: bar lists, and grids (shades, or with `--values` the time too),
//! wrapped to the width. Numbers come from `stats::layout`, labels from `label`.

use std::ops::Range;

use chrono::Local;
use crossterm::terminal;

use super::{Part, Section, StatsReport, span_title};
use crate::{
    model::time::{Minutes, Time},
    stats::{
        DimSpec, KeyPart, Query, Row,
        dims::calendar::Unit,
        layout::{Grid, Layout, label_in_span, layout},
        minutes_of,
    },
};

/// Width of a piped report, and of a terminal that does not say.
const PIPED_WIDTH: usize = 80;
/// Cells of the bar of the largest row.
const BAR_CELLS: usize = 20;
/// Indent of the parts under a heading.
const INDENT: usize = 2;
const TOTAL: &str = "total";

/// The text of `report`. `terminal` says whether stdout is one: then its
/// width counts, else 80 columns.
pub fn render(report: &StatsReport, terminal: bool) -> String {
    let width = if terminal {
        // A terminal that reports no columns (0) gets the piped width too.
        terminal::size()
            .ok()
            .filter(|&(columns, _)| columns > 0)
            .map_or(PIPED_WIDTH, |(columns, _)| usize::from(columns))
    } else {
        PIPED_WIDTH
    };
    report
        .sections
        .iter()
        .map(|section| section_text(section, report.values, report.now, width))
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// A section: its heading with the total, then its parts indented under it.
/// No time at all in the span: `no time tracked` under the heading instead.
fn section_text(section: &Section, values: bool, now: Time, width: usize) -> String {
    let Some(first) = section.parts.first() else {
        return section.title.clone();
    };
    let seconds = first.result.total.seconds;
    let total = minutes_of(seconds);
    let heading = if section.title == "last 7 days" {
        // The plain week (flagged sections are titled by their span): the
        // span, and the average day.
        let query = &first.result.query;
        let span = span_title(query.from, query.to, now);
        let average = minutes_of(seconds / 7);
        format!("{}  {span}  {total}  (avg {average}/day)", section.title)
    } else {
        format!("{}  {total}", section.title)
    };
    let budget = width.saturating_sub(INDENT);
    let body = if seconds == 0 {
        indent("no time tracked")
    } else {
        section
            .parts
            .iter()
            .map(|part| part_text(part, values, budget))
            .filter(|text| !text.is_empty())
            .map(|text| indent(&text))
            .collect::<Vec<_>>()
            .join("\n\n")
    };
    if body.is_empty() {
        heading
    } else {
        format!("{heading}\n{body}")
    }
}

/// `text` with each non-empty line indented.
fn indent(text: &str) -> String {
    text.lines()
        .map(|line| {
            if line.is_empty() {
                String::new()
            } else {
                format!("{}{line}", " ".repeat(INDENT))
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// One part, not indented: its list or grids; a total alone has no lines.
fn part_text(part: &Part, values: bool, width: usize) -> String {
    let query = &part.result.query;
    match layout(&part.result) {
        Layout::Total => String::new(),
        Layout::List { dim, rows } => list_text(&rows, dim, query),
        Layout::Grid(grid) => grid_text(&grid, query, values, width),
        Layout::Grids(grids) => grids
            .iter()
            .map(|(outer, grid)| {
                let heading = outer
                    .iter()
                    .map(|key| label_text(key, query))
                    .collect::<Vec<_>>()
                    .join(" / ");
                format!("{heading}\n{}", grid_text(grid, query, values, width))
            })
            .collect::<Vec<_>>()
            .join("\n\n"),
    }
}

/// The unit and count of the query's calendar dimension, if it has one.
fn calendar_of(query: &Query) -> Option<(Unit, u32)> {
    query.dims.iter().find_map(|dim| match dim {
        DimSpec::Calendar { unit, every } => Some((*unit, *every)),
        _ => None,
    })
}

/// A key part's label, in the local zone, for the query's unit and span.
fn label_text(key: &KeyPart, query: &Query) -> String {
    let (unit, every) = calendar_of(query).map_or((None, 1), |(unit, every)| (Some(unit), every));
    label_in_span(key, unit, every, query.from, query.to, &Local)
}

/// A list: label, time, bar; hour and weekday add the average per occurrence.
fn list_text(rows: &[Row], dim: &str, query: &Query) -> String {
    let cyclic = matches!(dim, "hour" | "weekday");
    let labels: Vec<String> = rows
        .iter()
        .map(|row| label_text(&row.key.0[0], query))
        .collect();
    let times: Vec<String> = rows
        .iter()
        .map(|row| minutes_of(row.seconds).to_string())
        .collect();
    let largest = rows
        .iter()
        .map(|row| minutes_of(row.seconds).get())
        .max()
        .unwrap_or(0);
    let label_width = widest(&labels);
    let time_width = widest(&times);
    rows.iter()
        .zip(labels.iter().zip(&times))
        .map(|(row, (label, time))| {
            let bar = bar_of(minutes_of(row.seconds).get(), largest);
            let mut line = format!("{label:<label_width$}  {time:>time_width$}  {bar:<BAR_CELLS$}");
            if cyclic && row.occurrences > 0 {
                let average = minutes_of(row.seconds / i64::from(row.occurrences));
                line += &format!("  (avg {average})");
            }
            line.trim_end().to_string()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The bar of `minutes` against the `largest` row: proportional, rounded
/// down, but a row with time has at least one cell.
fn bar_of(minutes: u32, largest: u32) -> String {
    let cells = match minutes {
        0 => 0,
        _ => (minutes as usize * BAR_CELLS / largest as usize).max(1),
    };
    "█".repeat(cells)
}

/// The width of the widest text, in characters.
fn widest(texts: &[String]) -> usize {
    texts
        .iter()
        .map(|text| text.chars().count())
        .max()
        .unwrap_or(0)
}

/// A grid: the column labels, a line per row (its cells, then its total),
/// the totals line, and the legend `█ = <largest cell>`. Cells are shades,
/// with `values` the shade and the time. Too wide for `width`: column blocks,
/// each with the row labels and its totals line (without `values`, only the
/// last block's, the one with numbers); the totals column in the last.
fn grid_text(grid: &Grid, query: &Query, values: bool, width: usize) -> String {
    let largest = grid.cells.iter().flatten().copied().max().unwrap_or(0);
    let cell = |minutes: u32| match minutes {
        0 => "·".to_string(),
        _ if values => format!("{}{}", shade(minutes, largest), Minutes::new(minutes)),
        _ => shade(minutes, largest).to_string(),
    };
    let cells: Vec<Vec<String>> = grid
        .cells
        .iter()
        .map(|row| row.iter().map(|&minutes| cell(minutes)).collect())
        .collect();
    let rows: Vec<String> = grid.rows.iter().map(|key| label_text(key, query)).collect();
    let cols: Vec<String> = grid.cols.iter().map(|key| label_text(key, query)).collect();
    let row_totals: Vec<String> = grid
        .row_totals
        .iter()
        .map(|&minutes| Minutes::new(minutes).to_string())
        .collect();
    let col_totals: Vec<String> = grid
        .col_totals
        .iter()
        .map(|&minutes| Minutes::new(minutes).to_string())
        .collect();
    let total = Minutes::new(grid.total).to_string();

    let label_width = widest(&rows).max(TOTAL.len());
    let total_width = widest(&row_totals)
        .max(total.chars().count())
        .max(TOTAL.len());
    // A column is as wide as its label, its cells and, with --values, its total.
    let col_widths: Vec<usize> = (0..cols.len())
        .map(|c| {
            let cells_width = cells
                .iter()
                .map(|row| row[c].chars().count())
                .max()
                .unwrap_or(0);
            let totals_width = if values {
                col_totals[c].chars().count()
            } else {
                0
            };
            cols[c].chars().count().max(cells_width).max(totals_width)
        })
        .collect();

    let blocks = column_blocks(&col_widths, label_width, total_width, width);
    let texts: Vec<String> = blocks
        .iter()
        .map(|block| {
            let last = block.end == cols.len();
            let mut lines = Vec::new();
            let mut header = " ".repeat(label_width);
            for c in block.clone() {
                header += &format!(" {:>w$}", cols[c], w = col_widths[c]);
            }
            if last {
                header += &format!(" {TOTAL:>total_width$}");
            }
            lines.push(header);
            for (r, row_label) in rows.iter().enumerate() {
                let mut line = format!("{row_label:<label_width$}");
                for c in block.clone() {
                    line += &format!(" {:>w$}", cells[r][c], w = col_widths[c]);
                }
                if last {
                    line += &format!(" {:>total_width$}", row_totals[r]);
                }
                lines.push(line);
            }
            let mut totals = format!("{TOTAL:<label_width$}");
            for c in block.clone() {
                let shown = if values { col_totals[c].as_str() } else { "" };
                totals += &format!(" {:>w$}", shown, w = col_widths[c]);
            }
            if last {
                totals += &format!(" {total:>total_width$}");
            }
            // Without --values a block but the last has no numbers in its totals line.
            if values || last {
                lines.push(totals);
            }
            lines
                .iter()
                .map(|line| line.trim_end())
                .collect::<Vec<_>>()
                .join("\n")
        })
        .collect();
    format!("{}\n\n█ = {}", texts.join("\n\n"), Minutes::new(largest))
}

/// The shade of a cell by its share of the largest: `░` under a quarter,
/// `▒` under half, `▓` under three quarters, `█` above.
fn shade(minutes: u32, largest: u32) -> char {
    if minutes * 4 < largest {
        '░'
    } else if minutes * 2 < largest {
        '▒'
    } else if minutes * 4 < largest * 3 {
        '▓'
    } else {
        '█'
    }
}

/// The column ranges of the blocks a grid of `widths` takes in `width`. Greedy:
/// each block takes as many columns as fit beside the row labels, and the last
/// one also the totals column. That leaves a stub block at the end, so when
/// the greedy count `n` is more than one, the columns are shared evenly over
/// `n` blocks (the earlier ones take the extra), if every block still fits.
/// A block is never empty, so a column wider than the line gets a block of its
/// own.
fn column_blocks(
    widths: &[usize],
    label_width: usize,
    total_width: usize,
    width: usize,
) -> Vec<Range<usize>> {
    let greedy = greedy_blocks(widths, label_width, total_width, width);
    let n = greedy.len();
    if n <= 1 {
        return greedy;
    }
    let base = widths.len() / n;
    let extra = widths.len() % n;
    let mut start = 0;
    let even: Vec<Range<usize>> = (0..n)
        .map(|i| {
            let end = start + base + usize::from(i < extra);
            let block = start..end;
            start = end;
            block
        })
        .collect();
    let fits = even.iter().enumerate().all(|(i, block)| {
        let totals = if i + 1 == n { 1 + total_width } else { 0 };
        let used: usize = widths[block.clone()].iter().map(|w| 1 + w).sum();
        label_width + used + totals <= width
    });
    if fits { even } else { greedy }
}

/// The blocks of `column_blocks` before the even split: as many columns as
/// fit, each block a column or more.
fn greedy_blocks(
    widths: &[usize],
    label_width: usize,
    total_width: usize,
    width: usize,
) -> Vec<Range<usize>> {
    let mut blocks = Vec::new();
    let mut start = 0;
    while start < widths.len() {
        let mut end = start;
        let mut used = label_width;
        while end < widths.len() && used + 1 + widths[end] <= width {
            used += 1 + widths[end];
            end += 1;
        }
        if end == widths.len() {
            if used + 1 + total_width <= width || end - start <= 1 {
                blocks.push(start..end);
                break;
            }
            // The totals column does not fit: the last column starts a block.
            end -= 1;
        }
        if end == start {
            end = start + 1;
        }
        blocks.push(start..end);
        start = end;
    }
    blocks
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        cli::{
            commands::stats::{ByDim, Every, StatsArgs, run},
            parse::SessionTime,
        },
        model::{
            id::NodeId,
            time::{Time, local_to_fixed},
        },
        stats::{NodeKey, dims::calendar::Unit},
        test_util::{core, dt, local, parse_time},
    };

    fn now() -> Time {
        local(20, 0)
    }

    /// 2026-10-`day` at `h:m`, local.
    fn on(day: u32, h: u32, m: u32) -> Time {
        local_to_fixed(dt(2026, 10, day, h, m)).unwrap()
    }

    fn args() -> StatsArgs {
        StatsArgs {
            node: None,
            from: None,
            to: None,
            every: None,
            by: vec![],
            values: false,
            deleted: false,
        }
    }

    /// The text of `args` over `sessions` on task "a" (in the root).
    async fn text_of(sessions: &[(Time, Time)], args: StatsArgs) -> String {
        let (tmp, core) = core().await;
        for (start, end) in sessions {
            core.add_session(&[0], *start, *end, now()).await.unwrap();
        }
        let report = run(&core, tmp.path(), now(), &args).await.unwrap();
        render(&report, false)
    }

    /// Flags for one span, 9 to 15 October (the seven days before today).
    fn span(by: Vec<ByDim>, values: bool) -> StatsArgs {
        StatsArgs {
            from: Some(SessionTime::At(dt(2026, 10, 9, 0, 0))),
            to: Some(SessionTime::At(dt(2026, 10, 16, 0, 0))),
            by,
            values,
            ..args()
        }
    }

    #[tokio::test]
    async fn plain_text() {
        let sessions = [
            (on(15, 9, 0), on(15, 10, 0)), // today, 1h
            (on(9, 13, 0), on(9, 14, 20)), // Fri 9, 1h20
        ];

        let text = text_of(&sessions, args()).await;

        let bar = |n: usize| "█".repeat(n);
        let expected = [
            "today  1h".to_string(),
            format!("  (root)  1h  {}", bar(20)),
            String::new(),
            "last 7 days  Fri 9 Oct – Thu 15 Oct  2h20  (avg 20m/day)".to_string(),
            format!("  Fri 9   1h20  {}", bar(20)),
            "  Sat 10    0m".to_string(),
            "  Sun 11    0m".to_string(),
            "  Mon 12    0m".to_string(),
            "  Tue 13    0m".to_string(),
            "  Wed 14    0m".to_string(),
            format!("  Thu 15    1h  {}", bar(15)),
            String::new(),
            format!("  (root)  2h20  {}", bar(20)),
        ]
        .join("\n");
        assert_eq!(text, expected);
    }

    #[tokio::test]
    async fn list_bars_scale() {
        let sessions = [
            (on(9, 13, 0), on(9, 14, 20)), // Fri 9, 80m: the largest, 20 cells
            (on(13, 9, 0), on(13, 9, 10)), // Tue 13, 10m: 2.5 cells, rounded down
            (on(14, 9, 0), on(14, 9, 1)),  // Wed 14, 1m: at least one cell
            (on(15, 9, 0), on(15, 10, 0)), // Thu 15, 60m: 15 cells
        ];
        let mut args = span(vec![], false);
        args.every = Some(Every {
            unit: Unit::Day,
            count: 1,
        });

        let text = text_of(&sessions, args).await;

        let bar = |n: usize| "█".repeat(n);
        let expected = [
            "Fri 9 Oct – Thu 15 Oct  2h31".to_string(),
            format!("  Fri 9   1h20  {}", bar(20)),
            "  Sat 10    0m".to_string(),
            "  Sun 11    0m".to_string(),
            "  Mon 12    0m".to_string(),
            format!("  Tue 13   10m  {}", bar(2)),
            format!("  Wed 14    1m  {}", bar(1)),
            format!("  Thu 15    1h  {}", bar(15)),
        ]
        .join("\n");
        assert_eq!(text, expected);
    }

    #[tokio::test]
    async fn weekday_list_avg() {
        let sessions = [
            (on(8, 9, 0), on(8, 10, 0)),    // Thu 8, 60m
            (on(15, 9, 0), on(15, 10, 30)), // Thu 15, 90m
        ];
        // Oct 1 to Oct 15: three Thursdays, two of every other day.
        let mut args = span(vec![ByDim::Weekday], false);
        args.from = Some(SessionTime::At(dt(2026, 10, 1, 0, 0)));

        let text = text_of(&sessions, args).await;

        let line = |day: &str, time: &str, bar: usize, avg: &str| {
            format!(
                "  {day:<3}  {time:>4}  {:<20}  (avg {avg})",
                "█".repeat(bar)
            )
        };
        let expected = [
            "Thu 1 Oct – Thu 15 Oct  2h30".to_string(),
            line("Mon", "0m", 0, "0m"),
            line("Tue", "0m", 0, "0m"),
            line("Wed", "0m", 0, "0m"),
            line("Thu", "2h30", 20, "50m"),
            line("Fri", "0m", 0, "0m"),
            line("Sat", "0m", 0, "0m"),
            line("Sun", "0m", 0, "0m"),
        ]
        .join("\n");
        assert_eq!(text, expected);
    }

    #[tokio::test]
    async fn grid_shades() {
        // Daily project grid: the largest day is 1h40, so 10m is ░, 30m ▒,
        // 70m ▓, 90m █; 24m ░; the days without time are ·.
        let sessions = [
            (on(8, 9, 0), on(8, 9, 10)),
            (on(9, 9, 0), on(9, 9, 30)),
            (on(10, 9, 0), on(10, 10, 10)),
            (on(11, 9, 0), on(11, 10, 30)),
            (on(13, 9, 0), on(13, 9, 24)),
            (on(15, 9, 0), on(15, 10, 40)),
        ];
        let mut args = span(vec![], false);
        args.from = Some(SessionTime::At(dt(2026, 10, 8, 0, 0)));
        args.every = Some(Every {
            unit: Unit::Day,
            count: 1,
        });
        args.by = vec![ByDim::Project];

        let text = text_of(&sessions, args).await;

        let row = |day: &str, cell: &str, total: &str| format!("  {day:<6} {cell:>6} {total:>5}");
        let expected = [
            "Thu 8 Oct – Thu 15 Oct  5h24".to_string(),
            row("", "(root)", "total"),
            row("Thu 8", "░", "10m"),
            row("Fri 9", "▒", "30m"),
            row("Sat 10", "▓", "1h10"),
            row("Sun 11", "█", "1h30"),
            row("Mon 12", "·", "0m"),
            row("Tue 13", "░", "24m"),
            row("Wed 14", "·", "0m"),
            row("Thu 15", "█", "1h40"),
            row("total", "", "5h24"),
            String::new(),
            "  █ = 1h40".to_string(),
        ]
        .join("\n");
        assert_eq!(text, expected);
    }

    #[tokio::test]
    async fn values_grid_cells() {
        let sessions = [
            (on(8, 9, 0), on(8, 9, 10)),
            (on(9, 9, 0), on(9, 9, 30)),
            (on(10, 9, 0), on(10, 10, 10)),
            (on(11, 9, 0), on(11, 10, 30)),
            (on(13, 9, 0), on(13, 9, 24)),
            (on(15, 9, 0), on(15, 10, 40)),
        ];
        let mut args = span(vec![], true);
        args.from = Some(SessionTime::At(dt(2026, 10, 8, 0, 0)));
        args.every = Some(Every {
            unit: Unit::Day,
            count: 1,
        });
        args.by = vec![ByDim::Project];

        let text = text_of(&sessions, args).await;

        let row = |day: &str, cell: &str, total: &str| format!("  {day:<6} {cell:>6} {total:>5}");
        let expected = [
            "Thu 8 Oct – Thu 15 Oct  5h24".to_string(),
            row("", "(root)", "total"),
            row("Thu 8", "░10m", "10m"),
            row("Fri 9", "▒30m", "30m"),
            row("Sat 10", "▓1h10", "1h10"),
            row("Sun 11", "█1h30", "1h30"),
            row("Mon 12", "·", "0m"),
            row("Tue 13", "░24m", "24m"),
            row("Wed 14", "·", "0m"),
            row("Thu 15", "█1h40", "1h40"),
            row("total", "5h24", "5h24"),
            String::new(),
            "  █ = 1h40".to_string(),
        ]
        .join("\n");
        assert_eq!(text, expected);
    }

    #[tokio::test]
    async fn values_grid_wraps_at_80() {
        let sessions = [
            (on(15, 9, 0), on(15, 10, 0)), // Thu 15, 9h
            (on(9, 9, 0), on(9, 9, 30)),   // Fri 9, 9h
        ];
        let args = span(vec![ByDim::Weekday, ByDim::Hour], true);

        let text = text_of(&sessions, args).await;

        assert!(
            text.lines().all(|line| line.chars().count() <= 80),
            "a line wider than 80:\n{text}"
        );
        let blocks = text
            .lines()
            .filter(|line| line.trim_start().starts_with("total"))
            .count();
        assert_eq!(blocks, 2, "the totals row under each block:\n{text}");
    }

    #[test]
    fn column_blocks_share_columns_evenly() {
        // Seven columns of 6 cells (a space and 5 wide) in 22 beside a label of
        // 4: greedy takes 3, 3 and a stub; evenly, 3, 2 and 2, all fitting.
        assert_eq!(column_blocks(&[5; 7], 4, 4, 22), vec![0..3, 3..5, 5..7]);
        // Everything in one block: one block.
        assert_eq!(column_blocks(&[2, 2, 2, 2], 3, 4, 80), vec![0..4]);
        // The even split, 4 and 4, puts the wide column with three others
        // (16 > 15): it does not fit, so greedy.
        assert_eq!(
            column_blocks(&[1, 1, 1, 1, 1, 1, 1, 9], 0, 0, 15),
            vec![0..7, 7..8]
        );
        // The totals column does not fit the last block: the last column
        // starts a block of its own.
        assert_eq!(column_blocks(&[3, 3, 3], 0, 5, 8), vec![0..2, 2..3]);
        // A column wider than the line gets a block of its own.
        assert_eq!(column_blocks(&[2, 30, 2], 1, 1, 10), vec![0..1, 1..2, 2..3]);
    }

    #[tokio::test]
    async fn values_grid_blocks_are_even() {
        // Four weeks, 17 September to 15 October (the fixed `now` is 15 October,
        // so no session may be later). Every hour has a Monday cell of 1h10
        // (50 and 20 minutes in two Mondays), so every column is wide. Monday 9
        // is the largest, at 1h35, so 1h10 is 74 % of it and shades ▓. Thursday
        // adds 10 minute cells for some shade variety.
        let sep = |day: u32, h: u32, m: u32| local_to_fixed(dt(2026, 9, day, h, m)).unwrap();
        let mut sessions = Vec::new();
        for hour in 0..24 {
            sessions.push((sep(21, hour, 0), sep(21, hour, 50)));
            sessions.push((on(5, hour, 0), on(5, hour, 20)));
            sessions.push((sep(17, hour, 0), sep(17, hour, 10)));
        }
        sessions.push((on(12, 9, 0), on(12, 9, 25)));
        let mut args = span(vec![ByDim::Weekday, ByDim::Hour], true);
        args.from = Some(SessionTime::At(dt(2026, 9, 17, 0, 0)));
        args.to = Some(SessionTime::At(dt(2026, 10, 15, 0, 0)));

        let text = text_of(&sessions, args).await;

        assert!(text.contains("▓1h10"), "{text}");
        assert!(text.contains("█1h35"), "{text}");
        assert!(
            text.lines().all(|line| line.chars().count() <= 80),
            "a line wider than 80:\n{text}"
        );
        // A block's header starts with its first hour; a totals row ends each.
        let headers: Vec<&str> = text
            .lines()
            .filter(|line| line.trim_start().starts_with(|c: char| c.is_ascii_digit()))
            .collect();
        let totals = text
            .lines()
            .filter(|line| line.trim_start().starts_with("total"))
            .count();
        assert!(
            totals >= 3,
            "the grid wraps into three blocks or more:\n{text}"
        );
        assert_eq!(headers.len(), totals, "a totals row per block:\n{text}");
        // Only the last block's header has the totals column.
        for (i, header) in headers.iter().enumerate() {
            let last = i + 1 == headers.len();
            assert_eq!(header.trim_end().ends_with("total"), last, "{text}");
        }
        // Each block lists the seven weekdays.
        let weekdays = text
            .lines()
            .filter(|line| {
                line.starts_with("  ")
                    && ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"]
                        .iter()
                        .any(|day| line.trim_start().starts_with(day))
            })
            .count();
        assert_eq!(weekdays, 7 * totals, "{text}");
        // The 24 hours are shared evenly: the earlier blocks take the extra.
        let widths: Vec<usize> = headers
            .iter()
            .map(|header| {
                header
                    .split_whitespace()
                    .filter(|token| *token != TOTAL)
                    .count()
            })
            .collect();
        assert_eq!(widths.iter().sum::<usize>(), 24, "{text}");
        let (most, fewest) = (widths.iter().max(), widths.iter().min());
        assert!(
            most.unwrap() - fewest.unwrap() <= 1,
            "uneven blocks {widths:?}:\n{text}"
        );
    }

    #[test]
    fn wrapped_grid_without_values_has_no_bare_total() {
        // Six columns of 22 characters beside a label of 14: two to a block.
        let removed = |name: &str| {
            KeyPart::Node(NodeKey::RemovedTask {
                id: NodeId::new(),
                name: name.into(),
            })
        };
        let grid = Grid {
            rows: vec![removed("row")],
            cols: (1..=6)
                .map(|i| removed(&format!("lab number {i}")))
                .collect(),
            cells: vec![vec![60; 6]],
            row_totals: vec![360],
            col_totals: vec![60; 6],
            total: 360,
        };
        let query = Query {
            from: parse_time("2026-10-09T00:00:00+02:00"),
            to: parse_time("2026-10-16T00:00:00+02:00"),
            dims: vec![],
            measures: vec![],
            include_removed_tasks: true,
        };
        let totals = |text: &str| {
            text.lines()
                .filter(|line| line.trim_start().starts_with("total"))
                .map(str::to_string)
                .collect::<Vec<String>>()
        };

        // Without --values only the last block's totals line, the one with numbers.
        let plain = grid_text(&grid, &query, false, 80);
        let lines = totals(&plain);
        assert_eq!(lines.len(), 1, "{plain}");
        assert!(lines[0].ends_with("6h"), "{plain}");
        assert!(plain.lines().all(|line| line.trim() != "total"), "{plain}");

        // With --values every block keeps its totals line.
        let valued = grid_text(&grid, &query, true, 80);
        assert_eq!(totals(&valued).len(), 3, "{valued}");
    }

    #[tokio::test]
    async fn day_labels_name_the_month_across_months() {
        // Sat 28 Feb and Sun 1 Mar are both in the span: each names its month.
        let sessions = [(
            local_to_fixed(dt(2026, 3, 1, 9, 0)).unwrap(),
            local_to_fixed(dt(2026, 3, 1, 10, 0)).unwrap(),
        )];
        let mut args = args();
        args.from = Some(SessionTime::At(dt(2026, 2, 28, 0, 0)));
        args.to = Some(SessionTime::At(dt(2026, 3, 2, 0, 0)));
        args.every = Some(Every {
            unit: Unit::Day,
            count: 1,
        });

        let text = text_of(&sessions, args).await;

        assert!(
            text.lines().any(|line| line.starts_with("  Sat 28 Feb")),
            "{text}"
        );
        assert!(
            text.lines().any(|line| line.starts_with("  Sun 1 Mar")),
            "{text}"
        );
    }

    #[tokio::test]
    async fn no_time_tracked() {
        let text = text_of(&[], args()).await;

        let expected = [
            "today  0m".to_string(),
            "  no time tracked".to_string(),
            String::new(),
            "last 7 days  Fri 9 Oct – Thu 15 Oct  0m  (avg 0m/day)".to_string(),
            "  no time tracked".to_string(),
        ]
        .join("\n");
        assert_eq!(text, expected);
    }

    #[tokio::test]
    async fn root_and_removed_labels() {
        let (tmp, mut core) = core().await;
        core.add_session(&[1, 0], on(15, 9, 0), on(15, 10, 0), now())
            .await
            .unwrap(); // "b", in the workspace: no project, so (root)
        core.add_session(&[0], on(15, 11, 0), on(15, 12, 0), now())
            .await
            .unwrap(); // "a", then removed
        core.delete(&[0], now()).await.unwrap();
        let args = StatsArgs {
            by: vec![ByDim::Project],
            deleted: true,
            ..args()
        };

        let report = run(&core, tmp.path(), now(), &args).await.unwrap();
        let text = render(&report, false);

        let bar = "█".repeat(20);
        let expected = [
            "Fri 9 Oct – Thu 15 Oct  2h".to_string(),
            format!("  {:<9}  {:>2}  {bar}", "(root)", "1h"),
            format!("  {:<9}  {:>2}  {bar}", "(removed)", "1h"),
        ]
        .join("\n");
        assert_eq!(text, expected);
    }
}
