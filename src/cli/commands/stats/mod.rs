//! `udo stats [NODE] [--from T] [--to T] [--every UNIT] [--by DIM,…] [--values]
//! [--deleted]`: where the time went. Plain: today and the last 7 days,
//! by project; with a flag: one span, grouped by what the flags name.

mod text;

use std::{collections::BTreeMap, fmt, io::IsTerminal, path::Path};

use chrono::{Datelike, Days, Local, NaiveDate, TimeDelta, Weekday};
use serde::Serialize;
use serde_json::{Map, Value, json};

use crate::{
    Res,
    cli::{
        parse::{SessionTime, session_time},
        report::Report,
        resolve::resolve,
    },
    core::Core,
    model::time::Time,
    stats::{
        DimSpec, KeyPart, MeasureSpec, NodeKey, Query, Row, Rows,
        dims::calendar::{Unit, midnight},
        layout::DAY_MONTH,
        spec::Level,
    },
};

#[derive(clap::Args)]
pub struct StatsArgs {
    /// A task, or a container for every task below it; default: the node
    /// of the current folder, else everything
    pub node: Option<String>,
    /// Start of the span (default: 00:00 six days ago)
    #[arg(long, value_parser = session_time, allow_hyphen_values = true)]
    pub from: Option<SessionTime>,
    /// End of the span (default: tomorrow 00:00); +D counts from --from
    #[arg(long, value_parser = session_time, allow_hyphen_values = true)]
    pub to: Option<SessionTime>,
    /// Buckets of a calendar unit: day, week, month, year, with a count (2weeks)
    #[arg(long, value_parser = every)]
    pub every: Option<Every>,
    /// Group by these, in this order: hour, weekday, task, project, workspace, child
    #[arg(long, value_enum, value_delimiter = ',')]
    pub by: Vec<ByDim>,
    /// Grid cells with their time (text only)
    #[arg(long)]
    pub values: bool,
    /// Also sessions of removed tasks
    #[arg(long)]
    pub deleted: bool,
}

/// `--every`: a calendar unit and how many of them make one bucket.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Every {
    unit: Unit,
    count: u32,
}

impl From<Every> for DimSpec {
    fn from(every: Every) -> Self {
        DimSpec::Calendar {
            unit: every.unit,
            every: every.count,
        }
    }
}

/// `--every`: `day`, `week`, `month` or `year` (singular or plural), with an
/// optional count in front: `2weeks`, `6months`.
fn every(input: &str) -> Result<Every, String> {
    let bad = || {
        format!(
            "invalid --every {input:?}: use day, week, month, year, \
             with an optional count: 2weeks, 6months"
        )
    };
    let digits = input
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(input.len());
    let (count, unit) = input.split_at(digits);
    let count = if count.is_empty() {
        1
    } else {
        count
            .parse::<u32>()
            .ok()
            .filter(|n| *n >= 1)
            .ok_or_else(bad)?
    };
    let unit = match unit {
        "day" | "days" => Unit::Day,
        "week" | "weeks" => Unit::Week,
        "month" | "months" => Unit::Month,
        "year" | "years" => Unit::Year,
        _ => return Err(bad()),
    };
    Ok(Every { unit, count })
}

/// `--by`: one dimension of the grouping.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum ByDim {
    Hour,
    Weekday,
    Task,
    Project,
    Workspace,
    Child,
}

impl From<ByDim> for DimSpec {
    fn from(by: ByDim) -> Self {
        match by {
            ByDim::Hour => DimSpec::Hour,
            ByDim::Weekday => DimSpec::Weekday,
            ByDim::Task => DimSpec::Node { level: Level::Task },
            ByDim::Project => DimSpec::Node {
                level: Level::Project,
            },
            ByDim::Workspace => DimSpec::Node {
                level: Level::Workspace,
            },
            ByDim::Child => DimSpec::Node {
                level: Level::Child,
            },
        }
    }
}

/// The stats as sections; each section has one or more parts (queries).
#[derive(Serialize)]
pub struct StatsReport {
    pub sections: Vec<Section>,
    /// `--values`: the grid cells with their time. Text only.
    #[serde(skip)]
    pub values: bool,
    /// The time of the run: the text names the year of a span that is not
    /// this one. Text only.
    #[serde(skip)]
    pub now: Time,
}

#[derive(Serialize)]
pub struct Section {
    pub title: String,
    pub parts: Vec<Part>,
}

/// One query with its total and rows.
#[derive(Serialize)]
pub struct Part {
    pub query: Query,
    pub total: RowJson,
    pub rows: Vec<RowJson>,
    /// The engine's rows, for the text output (`stats::layout`). Not in JSON.
    #[serde(skip)]
    pub result: Rows,
}

/// A row as `--json` shows it: the key with one field per dimension, the
/// measures flat, and the occurrences.
#[derive(Serialize)]
pub struct RowJson {
    pub key: Map<String, Value>,
    #[serde(flatten)]
    pub values: BTreeMap<&'static str, Value>,
    pub occurrences: u32,
}

impl RowJson {
    fn of(row: &Row, dims: &[DimSpec]) -> Self {
        Self {
            key: dims
                .iter()
                .zip(&row.key.0)
                .map(|(dim, part)| (dim.name().to_string(), key_json(part)))
                .collect(),
            values: row.values.clone(),
            occurrences: row.occurrences,
        }
    }
}

impl Part {
    fn of(result: Rows) -> Self {
        let dims = &result.query.dims;
        Self {
            query: result.query.clone(),
            total: RowJson::of(&result.total, dims),
            rows: result
                .rows
                .iter()
                .map(|row| RowJson::of(row, dims))
                .collect(),
            result,
        }
    }
}

/// One key part in JSON: the shapes in the spec's "JSON" section.
fn key_json(part: &KeyPart) -> Value {
    match part {
        KeyPart::Calendar { start, end } => json!({ "start": start, "end": end }),
        KeyPart::Hour(hour) => json!(hour),
        KeyPart::Weekday(day) => json!(weekday_name(*day)),
        KeyPart::Node(NodeKey::Node { id, path, .. }) => json!({ "id": id, "path": path }),
        KeyPart::Node(NodeKey::RemovedTask { id, name }) => {
            json!({ "id": id, "name": name, "removed": true })
        }
        KeyPart::Node(NodeKey::Removed) => json!({ "removed": true }),
    }
}

fn weekday_name(day: Weekday) -> &'static str {
    match day {
        Weekday::Mon => "mon",
        Weekday::Tue => "tue",
        Weekday::Wed => "wed",
        Weekday::Thu => "thu",
        Weekday::Fri => "fri",
        Weekday::Sat => "sat",
        Weekday::Sun => "sun",
    }
}

/// A span title's date with its year: `Mon 19 Oct 2025`.
const DAY_MONTH_YEAR: &str = "%a %-d %b %Y";

/// `Mon 19 Oct – Fri 30 Oct`: the local days of the span. `to` is excluded,
/// so the day of its last second is the last day shown. The year shows on
/// both days when the span crosses a year, on the last day alone when the
/// span is in one other year, and nowhere in `now`'s year.
fn span_title(from: Time, to: Time, now: Time) -> String {
    let first = from.with_timezone(&Local).date_naive();
    let last = (to - TimeDelta::seconds(1))
        .with_timezone(&Local)
        .date_naive();
    let crosses = first.year() != last.year();
    let this_year = now.with_timezone(&Local).year();
    let show = |day: NaiveDate, year: bool| {
        let format = if year { DAY_MONTH_YEAR } else { DAY_MONTH };
        day.format(format).to_string()
    };
    format!(
        "{} – {}",
        show(first, crosses),
        show(last, crosses || last.year() != this_year)
    )
}

fn query(from: Time, to: Time, dims: Vec<DimSpec>, deleted: bool) -> Query {
    Query {
        from,
        to,
        dims,
        measures: vec![MeasureSpec::Minutes, MeasureSpec::Sessions],
        include_removed_tasks: deleted,
    }
}

/// One part: the stats of `query` for the node at `scope`.
async fn part(core: &Core, scope: &[usize], now: Time, query: Query) -> Res<Part> {
    Ok(Part::of(core.stats(&query, scope, now).await?))
}

/// The stats the flags ask for, with `now` and the local zone from the caller.
pub async fn run(core: &Core, cwd: &Path, now: Time, args: &StatsArgs) -> Res<StatsReport> {
    let tree = core.tree();
    let scope = match &args.node {
        Some(node) => resolve(tree, Some(node), cwd)?,
        None => resolve(tree, None, cwd).unwrap_or_default(), // outside udo: everything
    };
    let today = now.with_timezone(&Local).date_naive();
    let tomorrow = midnight(today + Days::new(1), &Local);
    let project = DimSpec::Node {
        level: Level::Project,
    };

    let plain =
        args.from.is_none() && args.to.is_none() && args.every.is_none() && args.by.is_empty();
    let sections = if plain {
        let week = midnight(today - Days::new(6), &Local);
        let day = DimSpec::Calendar {
            unit: Unit::Day,
            every: 1,
        };
        vec![
            Section {
                title: "today".into(),
                parts: vec![
                    part(
                        core,
                        &scope,
                        now,
                        query(
                            midnight(today, &Local),
                            tomorrow,
                            vec![project.clone()],
                            args.deleted,
                        ),
                    )
                    .await?,
                ],
            },
            Section {
                title: "last 7 days".into(),
                parts: vec![
                    part(
                        core,
                        &scope,
                        now,
                        query(week, tomorrow, vec![day], args.deleted),
                    )
                    .await?,
                    part(
                        core,
                        &scope,
                        now,
                        query(week, tomorrow, vec![project], args.deleted),
                    )
                    .await?,
                ],
            },
        ]
    } else {
        // `+D` in --to counts from an explicit --from only
        let from_arg = args.from.map(|t| t.resolve(now, None)).transpose()?;
        let to_arg = args.to.map(|t| t.resolve(now, from_arg)).transpose()?;
        let from = match from_arg {
            Some(from) => from,
            None => midnight(today - Days::new(6), &Local),
        };
        let to = to_arg.unwrap_or(tomorrow);
        if from >= to {
            return Err(match from_arg {
                Some(_) => "--from must be before --to",
                None => "--to must be after --from (default: 00:00 six days ago)",
            }
            .into());
        }

        // --every first, then --by in order: the key's order
        let dims: Vec<DimSpec> = args
            .every
            .map(DimSpec::from)
            .into_iter()
            .chain(args.by.iter().map(|by| DimSpec::from(*by)))
            .collect();
        for (i, dim) in dims.iter().enumerate() {
            if dims[..i].iter().any(|d| d.name() == dim.name()) {
                return Err(format!("--by lists {} twice", dim.name()).into());
            }
        }

        vec![Section {
            title: span_title(from, to, now),
            parts: vec![part(core, &scope, now, query(from, to, dims, args.deleted)).await?],
        }]
    };
    Ok(StatsReport {
        sections,
        values: args.values,
        now,
    })
}

impl fmt::Display for StatsReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&text::render(self, std::io::stdout().is_terminal()))
    }
}

impl Report for StatsReport {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        cli::{parse::SessionTime, report::render},
        model::time::local_to_fixed,
        test_util::{core, dt, local},
    };

    fn now() -> Time {
        local(20, 0)
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

    /// The report as `--json` prints it, parsed.
    fn json_of(report: &StatsReport) -> Value {
        serde_json::from_str(&render(report, true).unwrap()).unwrap()
    }

    /// The time in the JSON string `value`.
    fn time_at(value: &Value) -> Time {
        value.as_str().unwrap().parse().unwrap()
    }

    #[tokio::test]
    async fn plain_has_two_sections() {
        let (tmp, core) = core().await;

        let report = run(&core, tmp.path(), now(), &args()).await.unwrap();

        let titles: Vec<&str> = report.sections.iter().map(|s| s.title.as_str()).collect();
        assert_eq!(titles, ["today", "last 7 days"]);
        let parts: Vec<usize> = report.sections.iter().map(|s| s.parts.len()).collect();
        assert_eq!(parts, [1, 2]);
    }

    fn ev(unit: Unit, count: u32) -> Result<Every, String> {
        Ok(Every { unit, count })
    }

    #[test]
    fn every_parse() {
        assert_eq!(every("day"), ev(Unit::Day, 1));
        assert_eq!(every("weeks"), ev(Unit::Week, 1));
        assert_eq!(every("2weeks"), ev(Unit::Week, 2));
        assert_eq!(every("6months"), ev(Unit::Month, 6));
        assert_eq!(every("1year"), ev(Unit::Year, 1));

        for bad in ["fortnight", "0weeks", "2", "", "2 weeks", "-1day"] {
            let err = every(bad).unwrap_err();
            assert!(
                err.contains("day, week, month, year, with an optional count: 2weeks, 6months"),
                "{err}"
            );
        }
    }

    #[tokio::test]
    async fn by_order_is_key_order() {
        let (tmp, core) = core().await;
        let args = StatsArgs {
            every: Some(Every {
                unit: Unit::Day,
                count: 1,
            }),
            by: vec![ByDim::Hour, ByDim::Weekday],
            ..args()
        };

        let report = run(&core, tmp.path(), now(), &args).await.unwrap();

        let dims: Vec<&str> = report.sections[0].parts[0]
            .query
            .dims
            .iter()
            .map(DimSpec::name)
            .collect();
        assert_eq!(dims, ["day", "hour", "weekday"]);
    }

    #[tokio::test]
    async fn by_repeated_is_refused() {
        let (tmp, core) = core().await;
        let args = StatsArgs {
            by: vec![ByDim::Hour, ByDim::Hour],
            ..args()
        };

        let Err(err) = run(&core, tmp.path(), now(), &args).await else {
            panic!("--by hour,hour was accepted");
        };

        assert!(err.to_string().contains("--by lists hour twice"), "{err}");
    }

    #[tokio::test]
    async fn from_after_to_is_error() {
        let (tmp, core) = core().await;
        let args = StatsArgs {
            from: Some(SessionTime::At(dt(2026, 10, 16, 0, 0))),
            to: Some(SessionTime::At(dt(2026, 10, 15, 0, 0))),
            ..args()
        };

        let Err(err) = run(&core, tmp.path(), now(), &args).await else {
            panic!("--from after --to was accepted");
        };

        assert!(
            err.to_string().contains("--from must be before --to"),
            "{err}"
        );
    }

    #[tokio::test]
    async fn to_before_defaulted_from_is_error() {
        let (tmp, core) = core().await;
        // The default from is 00:00 six days ago: 9 October at `now`.
        for to in [dt(2026, 10, 1, 0, 0), dt(2026, 10, 9, 0, 0)] {
            let args = StatsArgs {
                to: Some(SessionTime::At(to)),
                ..args()
            };

            let Err(err) = run(&core, tmp.path(), now(), &args).await else {
                panic!("--to {to} was accepted");
            };

            assert!(
                err.to_string()
                    .contains("--to must be after --from (default: 00:00 six days ago)"),
                "{err}"
            );
        }
    }

    #[tokio::test]
    async fn equal_from_and_to_is_error() {
        let (tmp, core) = core().await;
        let args = StatsArgs {
            from: Some(SessionTime::At(dt(2026, 10, 15, 0, 0))),
            to: Some(SessionTime::At(dt(2026, 10, 15, 0, 0))),
            ..args()
        };

        let Err(err) = run(&core, tmp.path(), now(), &args).await else {
            panic!("--from equal to --to was accepted");
        };

        assert!(
            err.to_string().contains("--from must be before --to"),
            "{err}"
        );
    }

    #[tokio::test]
    async fn json_shape_plain() {
        let (tmp, core) = core().await;
        core.add_session(&[0], local(9, 0), local(10, 0), now())
            .await
            .unwrap();

        let report = run(&core, tmp.path(), now(), &args()).await.unwrap();
        let json = json_of(&report);

        assert_eq!(json["sections"][0]["title"], "today");
        let today = &json["sections"][0]["parts"][0];
        assert_eq!(today["total"]["key"], json!({}));
        assert_eq!(today["total"]["minutes"], 60);
        assert_eq!(today["total"]["sessions"], 1);
        assert_eq!(today["total"]["occurrences"], 1);
        let by_project = &today["rows"][0];
        assert_eq!(by_project["key"]["project"]["path"], "");
        assert!(by_project["key"]["project"]["id"].is_string());

        let week = &json["sections"][1]["parts"][0];
        let days = week["rows"].as_array().unwrap();
        assert_eq!(days.len(), 7);
        let day = &days[6]; // 2026-10-15, the last of the seven
        assert_eq!(time_at(&day["key"]["day"]["start"]), local(0, 0));
        assert_eq!(
            time_at(&day["key"]["day"]["end"]),
            local(0, 0) + TimeDelta::days(1)
        );
        assert_eq!(day["minutes"], 60);
        assert_eq!(day["occurrences"], 1);
    }

    #[test]
    fn span_title_names_years_when_not_obvious() {
        let day = |y, m, d| local_to_fixed(dt(y, m, d, 0, 0)).unwrap();
        let now = day(2026, 10, 31);

        // This year: no year.
        assert_eq!(
            span_title(day(2026, 10, 19), day(2026, 10, 31), now),
            "Mon 19 Oct – Fri 30 Oct"
        );
        // One other year, all of the span in it: the year on the last day.
        assert_eq!(
            span_title(day(2025, 10, 19), day(2025, 10, 31), now),
            "Sun 19 Oct – Thu 30 Oct 2025"
        );
        // Two years: the year on both days.
        assert_eq!(
            span_title(day(2025, 1, 1), day(2026, 11, 1), now),
            "Wed 1 Jan 2025 – Sat 31 Oct 2026"
        );
    }

    #[tokio::test]
    async fn json_shape_grid() {
        let (tmp, core) = core().await;
        core.add_session(&[0], local(9, 0), local(10, 0), now())
            .await
            .unwrap();
        let args = StatsArgs {
            from: Some(SessionTime::At(dt(2026, 10, 15, 0, 0))),
            to: Some(SessionTime::At(dt(2026, 10, 16, 0, 0))),
            every: Some(Every {
                unit: Unit::Week,
                count: 1,
            }),
            by: vec![ByDim::Weekday, ByDim::Hour],
            ..args()
        };

        let report = run(&core, tmp.path(), now(), &args).await.unwrap();
        let json = json_of(&report);

        let part = &json["sections"][0]["parts"][0];
        // The query echo: the shape `--json` has shipped.
        let query = &part["query"];
        assert_eq!(query["dims"][0]["dim"], "calendar");
        assert_eq!(query["dims"][0]["unit"], "week");
        assert_eq!(query["dims"][0]["every"], 1);
        assert_eq!(query["dims"][1]["dim"], "weekday");
        assert_eq!(query["dims"][2]["dim"], "hour");
        assert_eq!(time_at(&query["from"]), local(0, 0));
        assert_eq!(time_at(&query["to"]), local(0, 0) + TimeDelta::days(1));
        assert_eq!(query["measures"], json!(["minutes", "sessions"]));
        assert_eq!(query["include_removed_tasks"], false);
        assert_eq!(part["total"]["key"], json!({}));
        let busy = part["rows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["key"]["hour"] == 9 && row["key"]["weekday"] == "thu")
            .unwrap();
        assert_eq!(time_at(&busy["key"]["week"]["start"]), local(0, 0));
        assert_eq!(busy["minutes"], 60);
        assert_eq!(busy["sessions"], 1);
        assert_eq!(busy["occurrences"], 1);
    }

    /// A removed task: its own key per task, one `removed` group above it.
    #[tokio::test]
    async fn json_key_of_removed_tasks() {
        let (tmp, mut core) = core().await;
        core.add_session(&[0], local(9, 0), local(10, 0), now())
            .await
            .unwrap();
        core.delete(&[0], now()).await.unwrap(); // task "a" is gone
        let args = StatsArgs {
            by: vec![ByDim::Task, ByDim::Project],
            deleted: true,
            ..args()
        };

        let report = run(&core, tmp.path(), now(), &args).await.unwrap();
        let json = json_of(&report);

        let row = &json["sections"][0]["parts"][0]["rows"][0];
        assert_eq!(row["key"]["task"]["name"], "a");
        assert_eq!(row["key"]["task"]["removed"], true);
        assert!(row["key"]["task"]["id"].is_string());
        assert_eq!(row["key"]["project"], json!({ "removed": true }));
    }
}
